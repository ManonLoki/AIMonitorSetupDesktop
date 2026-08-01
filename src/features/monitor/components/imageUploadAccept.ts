import type { ImageUploadAccept } from "../api/monitor";

// 将 Rust 返回的格式策略适配为 HTML file input 的 accept 属性。
export function imageUploadAcceptValue(
  policy: ImageUploadAccept | undefined,
): string {
  return policy ? [...policy.extensions, ...policy.mimeTypes].join(",") : "";
}
