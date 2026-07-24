import { queryOptions } from "@tanstack/react-query";
import { getSystemOverview } from "../api/system";

export const systemOverviewQuery = queryOptions({
  queryKey: ["system", "overview"],
  queryFn: getSystemOverview,
  staleTime: Number.POSITIVE_INFINITY,
});
