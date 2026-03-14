use std::collections::HashMap;
use std::io::{self, Write};
use std::time::{Duration, Instant};

use crossterm::{
    cursor, execute,
    style::Stylize,
    terminal,
};

use rustflow_core::step::{Step, StepKind, StepState};
use rustflow_orchestrator::{SchedulerEvent, compute_layers, flow_renderer::ExecutionLayer};

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const TICK_INTERVAL: Duration = Duration::from_millis(80);

/// Per-step display state.
#[derive(Debug, Clone)]
struct StepDisplay {
    name: String,
    kind_label: String,
    deps: Vec<String>,
    state: StepState,
    start_time: Option<Instant>,
    elapsed: Option<Duration>,
}

/// Manages in-place terminal rendering of workflow progress.
pub struct LiveProgress {
    layers: Vec<ExecutionLayer>,
    steps: HashMap<String, StepDisplay>,
    /// Total number of terminal lines occupied by the display.
    total_lines: usize,
    /// Whether the display has been initially printed.
    printed: bool,
    /// Spinner frame index.
    spinner_idx: usize,
    /// Workflow name for header.
    workflow_name: String,
    /// Total step count.
    step_count: usize,
    /// Workflow start time.
    start_time: Option<Instant>,
}

impl LiveProgress {
    pub fn new(steps: &[Step], workflow_name: &str) -> Self {
        let layers = compute_layers(steps);

        let mut step_displays = HashMap::new();

        for step in steps {
            let kind_label = match &step.kind {
                StepKind::Llm(cfg) => format!("{}/{}", cfg.provider, cfg.model),
                StepKind::Tool(cfg) => cfg.tool.clone(),
            };
            let deps: Vec<String> = step
                .depends_on
                .iter()
                .map(|d| d.as_str().to_string())
                .collect();

            step_displays.insert(
                step.id.as_str().to_string(),
                StepDisplay {
                    name: step.name.clone(),
                    kind_label,
                    deps,
                    state: StepState::Pending,
                    start_time: None,
                    elapsed: None,
                },
            );
        }

        // Compute display order and total lines.
        // Layout:
        //   header (1 line) + blank (1 line) = 2
        //   per layer: header(1) + steps(N) + footer(1) = N+2
        //   between layers: 2 lines (│ and ▼)
        //   trailing blank (1)
        let mut total_lines = 3; // header + blank + trailing blank
        for (i, layer) in layers.iter().enumerate() {
            total_lines += 1; // layer header
            total_lines += layer.step_ids.len(); // step lines
            total_lines += 1; // layer footer
            if i < layers.len() - 1 {
                total_lines += 2; // connector lines
            }
        }

        Self {
            layers,
            steps: step_displays,
            total_lines,
            printed: false,
            spinner_idx: 0,
            workflow_name: workflow_name.to_string(),
            step_count: steps.len(),
            start_time: None,
        }
    }

    /// Handle a scheduler event.
    pub fn on_event(&mut self, event: &SchedulerEvent) {
        match event {
            SchedulerEvent::StepStarted { step_id, .. } => {
                if let Some(sd) = self.steps.get_mut(step_id) {
                    sd.state = StepState::Running;
                    sd.start_time = Some(Instant::now());
                }
            }
            SchedulerEvent::StepSucceeded {
                step_id, elapsed, ..
            } => {
                if let Some(sd) = self.steps.get_mut(step_id) {
                    sd.state = StepState::Success;
                    sd.elapsed = Some(*elapsed);
                }
            }
            SchedulerEvent::StepFailed {
                step_id,
                will_retry,
                elapsed,
                ..
            } => {
                if let Some(sd) = self.steps.get_mut(step_id) {
                    sd.state = if *will_retry {
                        StepState::Retrying
                    } else {
                        StepState::Failed
                    };
                    if !will_retry {
                        sd.elapsed = Some(*elapsed);
                    }
                }
            }
            SchedulerEvent::StepRetrying { step_id, .. } => {
                if let Some(sd) = self.steps.get_mut(step_id) {
                    sd.state = StepState::Running;
                    sd.start_time = Some(Instant::now());
                }
            }
        }
    }

    /// Mark start time for the workflow.
    pub fn start(&mut self) {
        self.start_time = Some(Instant::now());
    }

    /// Render the full display. On first call, prints all lines.
    /// On subsequent calls, moves cursor up and overwrites.
    pub fn render(&mut self) {
        let mut stdout = io::stdout();

        if self.printed {
            // Move cursor up to the start of our display.
            execute!(stdout, cursor::MoveUp(self.total_lines as u16)).ok();
        }

        self.spinner_idx = (self.spinner_idx + 1) % SPINNER_FRAMES.len();

        let mut lines: Vec<String> = Vec::with_capacity(self.total_lines);

        // Header
        lines.push(format!(
            "  {}",
            format!(
                "╔═══ Workflow: {} ({} steps) ═══╗",
                self.workflow_name, self.step_count,
            )
            .cyan()
            .bold()
        ));
        lines.push(String::new());

        // Layers
        for (i, layer) in self.layers.iter().enumerate() {
            let is_parallel = layer.step_ids.len() > 1;
            let mode = if is_parallel { "parallel" } else { "serial" };

            lines.push(format!(
                "  {}",
                format!(
                    "┌─ Layer {} ── {} ({}) ─────────────────────",
                    layer.index + 1,
                    mode,
                    layer.step_ids.len(),
                )
                .dark_grey()
            ));

            for step_id in &layer.step_ids {
                if let Some(sd) = self.steps.get(step_id) {
                    lines.push(self.render_step_line(sd));
                }
            }

            lines.push(format!(
                "  {}",
                "└─────────────────────────────────────────────".dark_grey()
            ));

            if i < self.layers.len() - 1 {
                lines.push(format!("  {}", "               │".dark_grey()));
                lines.push(format!("  {}", "               ▼".dark_grey()));
            }
        }

        lines.push(String::new());

        // Write all lines, clearing each line first.
        for line in &lines {
            execute!(stdout, terminal::Clear(terminal::ClearType::CurrentLine)).ok();
            writeln!(stdout, "{line}").ok();
        }

        stdout.flush().ok();
        self.printed = true;
    }

    /// Render the final summary (non-animated).
    pub fn render_final(&self) {
        let mut stdout = io::stdout();

        if self.printed {
            execute!(stdout, cursor::MoveUp(self.total_lines as u16)).ok();
        }

        let mut lines: Vec<String> = Vec::with_capacity(self.total_lines);

        // Header
        lines.push(format!(
            "  {}",
            format!(
                "╔═══ Workflow: {} ({} steps) ═══╗",
                self.workflow_name, self.step_count,
            )
            .cyan()
            .bold()
        ));
        lines.push(String::new());

        // Layers
        for (i, layer) in self.layers.iter().enumerate() {
            let is_parallel = layer.step_ids.len() > 1;
            let mode = if is_parallel { "parallel" } else { "serial" };

            lines.push(format!(
                "  {}",
                format!(
                    "┌─ Layer {} ── {} ({}) ─────────────────────",
                    layer.index + 1,
                    mode,
                    layer.step_ids.len(),
                )
                .dark_grey()
            ));

            for step_id in &layer.step_ids {
                if let Some(sd) = self.steps.get(step_id) {
                    lines.push(self.render_final_step_line(sd));
                }
            }

            lines.push(format!(
                "  {}",
                "└─────────────────────────────────────────────".dark_grey()
            ));

            if i < self.layers.len() - 1 {
                lines.push(format!("  {}", "               │".dark_grey()));
                lines.push(format!("  {}", "               ▼".dark_grey()));
            }
        }

        lines.push(String::new());

        for line in &lines {
            execute!(stdout, terminal::Clear(terminal::ClearType::CurrentLine)).ok();
            writeln!(stdout, "{line}").ok();
        }

        // Total time
        if let Some(start) = self.start_time {
            let total = start.elapsed();
            writeln!(
                stdout,
                "  {} {:.1}s",
                "Total:".bold(),
                total.as_secs_f64()
            )
            .ok();
            writeln!(stdout).ok();
        }

        stdout.flush().ok();
    }

    /// Render a single step line with animated spinner for running steps.
    fn render_step_line(&self, sd: &StepDisplay) -> String {
        let (icon, elapsed_str) = match sd.state {
            StepState::Pending => {
                let icon = format!("{}", "○".dark_grey());
                (icon, String::new())
            }
            StepState::Running => {
                let frame = SPINNER_FRAMES[self.spinner_idx];
                let icon = format!("{}", frame.cyan());
                let elapsed = sd
                    .start_time
                    .map(|t| t.elapsed())
                    .unwrap_or(Duration::ZERO);
                let elapsed_str = format!(" {}", format!("{:.1}s", elapsed.as_secs_f64()).dark_grey());
                (icon, elapsed_str)
            }
            StepState::Success => {
                let icon = format!("{}", "✓".green());
                let elapsed_str = sd.elapsed.map_or(String::new(), |d| {
                    format!(" {}", format!("({:.1}s)", d.as_secs_f64()).dark_grey())
                });
                (icon, elapsed_str)
            }
            StepState::Failed => {
                let icon = format!("{}", "✗".red());
                let elapsed_str = sd.elapsed.map_or(String::new(), |d| {
                    format!(" {}", format!("({:.1}s)", d.as_secs_f64()).red())
                });
                (icon, elapsed_str)
            }
            StepState::Retrying => {
                let frame = SPINNER_FRAMES[self.spinner_idx];
                let icon = format!("{}", frame.yellow());
                let elapsed = sd
                    .start_time
                    .map(|t| t.elapsed())
                    .unwrap_or(Duration::ZERO);
                let elapsed_str = format!(
                    " {}",
                    format!("{:.1}s (retrying)", elapsed.as_secs_f64()).yellow()
                );
                (icon, elapsed_str)
            }
        };

        let deps_str = if sd.deps.is_empty() {
            String::new()
        } else {
            format!(" {} {}", "←".dark_grey(), sd.deps.join(", ").dark_grey())
        };

        let name_styled = match sd.state {
            StepState::Running => format!("{}", sd.name.clone().bold()),
            StepState::Success => format!("{}", sd.name.clone()),
            StepState::Failed => format!("{}", sd.name.clone().red()),
            _ => format!("{}", sd.name.clone().dark_grey()),
        };

        let kind_styled = format!("{}", format!("[{}]", sd.kind_label).dark_grey());

        format!(
            "  │  {icon} {name} {kind}{deps}{elapsed}",
            icon = icon,
            name = name_styled,
            kind = kind_styled,
            deps = deps_str,
            elapsed = elapsed_str,
        )
    }

    /// Render a final (non-animated) step line.
    fn render_final_step_line(&self, sd: &StepDisplay) -> String {
        let (icon, elapsed_str) = match sd.state {
            StepState::Success => {
                let icon = format!("{}", "✓".green());
                let elapsed_str = sd.elapsed.map_or(String::new(), |d| {
                    format!(" {}", format!("({:.1}s)", d.as_secs_f64()).dark_grey())
                });
                (icon, elapsed_str)
            }
            StepState::Failed => {
                let icon = format!("{}", "✗".red());
                let elapsed_str = sd.elapsed.map_or(String::new(), |d| {
                    format!(" {}", format!("({:.1}s)", d.as_secs_f64()).red())
                });
                (icon, elapsed_str)
            }
            _ => {
                let icon = format!("{}", "○".dark_grey());
                (icon, String::new())
            }
        };

        let deps_str = if sd.deps.is_empty() {
            String::new()
        } else {
            format!(" {} {}", "←".dark_grey(), sd.deps.join(", ").dark_grey())
        };

        let name_styled = match sd.state {
            StepState::Success => sd.name.clone(),
            StepState::Failed => format!("{}", sd.name.clone().red()),
            _ => format!("{}", sd.name.clone().dark_grey()),
        };

        let kind_styled = format!("{}", format!("[{}]", sd.kind_label).dark_grey());

        format!(
            "  │  {icon} {name} {kind}{deps}{elapsed}",
            icon = icon,
            name = name_styled,
            kind = kind_styled,
            deps = deps_str,
            elapsed = elapsed_str,
        )
    }

    /// Returns the tick interval for the render loop.
    pub fn tick_interval(&self) -> Duration {
        TICK_INTERVAL
    }

    /// Returns true if any step is still running.
    pub fn has_running(&self) -> bool {
        self.steps
            .values()
            .any(|s| s.state == StepState::Running || s.state == StepState::Retrying)
    }
}
