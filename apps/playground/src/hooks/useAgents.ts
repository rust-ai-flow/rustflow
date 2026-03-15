import { useCallback, useEffect, useState } from 'react';
import { client } from '../lib/client';
import type { AgentSummary } from '../types';

interface UseAgentsReturn {
  agents: AgentSummary[];
  loading: boolean;
  refresh: () => Promise<void>;
}

export function useAgents(autoRefreshMs = 5000): UseAgentsReturn {
  const [agents, setAgents] = useState<AgentSummary[]>([]);
  const [loading, setLoading] = useState(true);

  const refresh = useCallback(async () => {
    try {
      const { agents } = await client.listAgents();
      setAgents(agents);
    } catch {
      // Silently ignore network errors
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
    if (autoRefreshMs > 0) {
      const id = setInterval(() => void refresh(), autoRefreshMs);
      return () => clearInterval(id);
    }
  }, [refresh, autoRefreshMs]);

  return { agents, loading, refresh };
}
