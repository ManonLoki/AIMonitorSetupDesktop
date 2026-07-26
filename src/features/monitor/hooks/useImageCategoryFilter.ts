// 引入 React 的 useMemo/useState，用于派生计算与本地筛选状态
import { useMemo, useState } from "react";
// 引入远程图片的类型定义
import type { RemoteImage } from "../api/monitor";

// 图片分类筛选选项：全部，或按具体图片格式筛选
export type ImageCategory = "all" | "jpeg" | "png" | "gif";

// 根据当前选中的分类，对远程图片列表做筛选，并统计各格式数量
export function useImageCategoryFilter(images: RemoteImage[]) {
  // 当前选中的分类，默认展示全部
  const [category, setCategory] = useState<ImageCategory>("all");

  // 依赖 images 和 category 变化时才重新计算，避免每次渲染都重复过滤/统计
  const { filteredImages, counts } = useMemo(() => {
    // 各图片格式的数量统计初始值
    const counts = { jpeg: 0, png: 0, gif: 0 };
    for (const image of images) {
      // 把 mimeType（如 image/png）去掉前缀，得到格式名作为统计的 key
      const key = image.mimeType.replace("image/", "") as keyof typeof counts;
      // 只统计已知的三种格式，未知格式忽略
      if (key in counts) counts[key]++;
    }
    // 根据当前分类过滤图片列表：选“全部”时不过滤，否则按 mimeType 精确匹配
    const filteredImages =
      category === "all"
        ? images
        : images.filter((image) => image.mimeType === `image/${category}`);
    return { filteredImages, counts };
  }, [images, category]);

  // 对外暴露当前分类、切换分类的方法、过滤后的图片列表以及各格式的数量统计
  return { category, setCategory, filteredImages, counts };
}
