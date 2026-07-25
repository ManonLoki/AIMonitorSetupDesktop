import { useMemo, useState } from "react";
import type { RemoteImage } from "../api/monitor";

export type ImageCategory = "all" | "jpeg" | "png" | "gif";

export function useImageCategoryFilter(images: RemoteImage[]) {
  const [category, setCategory] = useState<ImageCategory>("all");

  const { filteredImages, counts } = useMemo(() => {
    const counts = { jpeg: 0, png: 0, gif: 0 };
    for (const image of images) {
      const key = image.mimeType.replace("image/", "") as keyof typeof counts;
      if (key in counts) counts[key]++;
    }
    const filteredImages =
      category === "all"
        ? images
        : images.filter((image) => image.mimeType === `image/${category}`);
    return { filteredImages, counts };
  }, [images, category]);

  return { category, setCategory, filteredImages, counts };
}
