import {
  Alert,
  Badge,
  Button,
  Card,
  Center,
  Group,
  Loader,
  Menu,
  SegmentedControl,
  SimpleGrid,
  Stack,
  Text,
} from "@mantine/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useRef, useState } from "react";
import {
  deleteRemoteImage,
  uploadRemoteImages,
} from "../api/monitor";
import {
  monitorKeys,
  remoteImagesQuery,
} from "../queries/monitor";
import { LineIcon } from "../../../shared/ui/LineIcon";

type ImageCategory = "all" | "jpeg" | "png" | "gif";

export function ImagesPage() {
  const inputRef = useRef<HTMLInputElement>(null);
  const queryClient = useQueryClient();
  const images = useQuery(remoteImagesQuery);
  const [category, setCategory] = useState<ImageCategory>("all");

  const upload = useMutation({
    mutationFn: uploadRemoteImages,
    onSettled: () =>
      queryClient.invalidateQueries({ queryKey: monitorKeys.images() }),
  });

  const remove = useMutation({
    mutationFn: deleteRemoteImage,
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: monitorKeys.images() }),
  });

  const error = images.error ?? upload.error ?? remove.error;
  const imageList = images.data ?? [];
  const filteredImages =
    category === "all"
      ? imageList
      : imageList.filter(
          (image) => image.mimeType === `image/${category}`,
        );
  const categoryCount = (value: Exclude<ImageCategory, "all">) =>
    imageList.filter((image) => image.mimeType === `image/${value}`).length;

  return (
    <Stack gap="lg">
      <Group justify="flex-end" gap="sm">
        <Button
          variant="default"
          leftSection={<LineIcon name="refresh" size={17} />}
          onClick={() => images.refetch()}
          loading={images.isFetching}
        >
          刷新
        </Button>
        <Button
          leftSection={<LineIcon name="upload" size={17} />}
          onClick={() => inputRef.current?.click()}
          loading={upload.isPending}
        >
          批量上传
        </Button>
        <input
          ref={inputRef}
          hidden
          type="file"
          multiple
          accept="image/jpeg,image/png,image/gif"
          onChange={(event) => {
            const files = Array.from(event.currentTarget.files ?? []);
            if (files.length > 0) upload.mutate(files);
            event.currentTarget.value = "";
          }}
        />
      </Group>

      {error && <Alert color="red">{error.message}</Alert>}
      {upload.isSuccess && (
        <Alert color="teal">
          已成功上传 {upload.data.length} 张图片。
        </Alert>
      )}

      {images.isPending ? (
        <Center py={80}>
          <Loader />
        </Center>
      ) : images.data?.length ? (
        <>
          <Group justify="space-between" align="center">
            <Text size="sm" c="dimmed">
              {category === "all"
                ? `共 ${imageList.length} 张图片`
                : `显示 ${filteredImages.length} / 共 ${imageList.length} 张`}
            </Text>
            <SegmentedControl
              size="sm"
              value={category}
              onChange={(value) => setCategory(value as ImageCategory)}
              data={[
                { value: "all", label: `全部 ${imageList.length}` },
                { value: "jpeg", label: `JPEG ${categoryCount("jpeg")}` },
                { value: "png", label: `PNG ${categoryCount("png")}` },
                { value: "gif", label: `GIF ${categoryCount("gif")}` },
              ]}
            />
          </Group>
          {filteredImages.length ? (
            <SimpleGrid
              cols={{ base: 1, xs: 2, md: 3, xl: 4 }}
              spacing="lg"
            >
              {filteredImages.map((image) => (
                <Card
                  key={image.filename}
                  withBorder
                  padding={0}
                  className="image-card"
                >
                  <div className="image-preview">
                    <img src={image.image} alt={image.filename} />
                    <Badge
                      className="image-type"
                      variant="filled"
                      color="dark"
                      size="xs"
                    >
                      {image.mimeType.replace("image/", "").toUpperCase()}
                    </Badge>
                  </div>
                  <Group p="md" justify="space-between" wrap="nowrap">
                    <div className="min-width-zero">
                      <Text fw={600} size="sm" truncate>
                        {image.filename}
                      </Text>
                      <Text c="dimmed" size="xs" mt={3}>
                        远端缓存
                      </Text>
                    </div>
                    <Menu shadow="md" position="bottom-end">
                      <Menu.Target>
                        <Button variant="subtle" color="gray" px={10}>
                          ···
                        </Button>
                      </Menu.Target>
                      <Menu.Dropdown>
                        <Menu.Item
                          color="red"
                          leftSection={<LineIcon name="trash" size={16} />}
                          onClick={() => remove.mutate(image.filename)}
                        >
                          删除图片
                        </Menu.Item>
                      </Menu.Dropdown>
                    </Menu>
                  </Group>
                </Card>
              ))}
            </SimpleGrid>
          ) : (
            <Center py={64}>
              <Text c="dimmed">该分类暂无图片</Text>
            </Center>
          )}
        </>
      ) : (
        <Card withBorder className="empty-state">
          <div className="empty-state-icon">
            <LineIcon name="image" size={30} />
          </div>
          <Text fw={650} mt="md">
            还没有远端图片
          </Text>
          <Text c="dimmed" size="sm" maw={360} ta="center" mt={6}>
            可一次选择多张 JPEG、PNG 或 GIF 图片，上传后直接分配给 AI
            实例。
          </Text>
          <Button
            mt="lg"
            variant="light"
            onClick={() => inputRef.current?.click()}
          >
            批量选择图片
          </Button>
        </Card>
      )}
    </Stack>
  );
}
