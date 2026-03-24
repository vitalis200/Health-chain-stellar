import { useQuery } from "@tanstack/react-query";
import { fetchRiders } from "@/lib/api/riders.api";
import { queryKeys } from "@/lib/api/queryKeys";

export function useRiders() {
  return useQuery({
    queryKey: ['riders', 'list'],
    queryFn: fetchRiders,
    refetchInterval: 5000, // Poll every 5 seconds for "real-time" feel
  });
}