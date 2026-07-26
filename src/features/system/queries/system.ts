// 引入 TanStack Query 的查询配置构造函数，用于生成可复用的查询选项对象
import { queryOptions } from "@tanstack/react-query";
// 引入系统概览的 API 调用函数（实际发起 Tauri invoke 的地方）
import { getSystemOverview } from "../api/system";

// 定义“系统概览”查询的配置：查询键、查询函数与缓存策略，供组件通过 useQuery 消费
export const systemOverviewQuery = queryOptions({
  queryKey: ["system", "overview"], // 缓存标识，用于区分/失效该查询
  queryFn: getSystemOverview, // 实际执行的数据获取函数
  staleTime: Number.POSITIVE_INFINITY, // 数据视为永不过期，避免重复请求（应用版本等信息不会变化）
});
