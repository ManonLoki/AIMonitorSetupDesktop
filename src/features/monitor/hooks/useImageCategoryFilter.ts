// 引入 React 的 useMemo/useState，用于派生计算与本地筛选状态
import { useMemo, useState } from "react";
// 引入远程图片的类型定义
import type { ImageFormat, RemoteImage } from "../api/monitor";

// 图片分类筛选选项：全部，或按具体图片格式筛选
export type ImageCategory = "all" | ImageFormat;

// 分类选择与列表过滤是纯展示状态；格式识别和数量聚合均由 Rust 完成。
export function useImageCategoryFilter(images: RemoteImage[]) {
  // 当前选中的分类，默认展示全部
  const [category, setCategory] = useState<ImageCategory>("all");

  // 依赖 images 和 category 变化时才重新计算，避免每次渲染都重复过滤/统计
  const filteredImages = useMemo(
    () =>
      category === "all"
        ? images
        : images.filter((image) => image.format === category),
    [images, category],
  );

  return { category, setCategory, filteredImages };
}
