import { useQuery } from "@tanstack/react-query";
import { fetchMe } from "../api/client";

export const ME_KEY = ["me"] as const;

export function useMe() {
  return useQuery({ queryKey: ME_KEY, queryFn: fetchMe, staleTime: 5 * 60_000 });
}
