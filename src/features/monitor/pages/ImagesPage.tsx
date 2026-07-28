// 引入 Mantine UI 组件：提示、徽标、按钮、卡片、居中容器、加载指示器、下拉菜单、分段控件等
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
// 引入 TanStack Query 的 mutation/query hooks 及 queryClient
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
// 引入 React 的引用与状态 hooks
import { useRef, useState } from "react";
// 引入图片相关的类型化 API 函数与类型
import {
  deleteRemoteImage,
  uploadRemoteImages,
  type RemoteImage,
} from "../api/monitor";
// 引入查询键与远端图片查询配置
import { monitorKeys, remoteImagesQuery } from "../queries/monitor";
// 引入按图片类型（jpeg/png/gif）过滤的本地 hook
import { useImageCategoryFilter } from "../hooks/useImageCategoryFilter";
// 引入通用图标组件
import { LineIcon } from "../../../shared/ui/LineIcon";
// 引入监控设备连接状态 hook
import { useMonitorConnection } from "../hooks/useMonitorConnection";
// 引入设备未就绪时的统一拦截组件
import { useMonitorDeviceGate } from "../components/MonitorDeviceGate";
import { useI18n } from "../../../shared/i18n";

export function ImagesPage() {
  const { t } = useI18n();
  // 隐藏的文件选择 input 的引用，用于以编程方式触发点击
  const inputRef = useRef<HTMLInputElement>(null);
  // 获取 QueryClient 实例，用于在 mutation 结束后使相关查询失效
  const queryClient = useQueryClient();
  // 获取当前监控设备的连接状态：设置、设备列表、是否已配置/可用设备、是否处于加载中
  const {
    settings,
    devices,
    hasConfiguredDevice,
    hasAvailableDevice,
    isPending: monitorPending,
  } = useMonitorConnection();
  // 查询远端图片列表（对接 list_remote_images 命令），仅在设备已配置且可用时才发起请求
  const images = useQuery({
    ...remoteImagesQuery,
    enabled: hasConfiguredDevice && hasAvailableDevice,
  });

  // 批量上传图片的 mutation：对接 upload_remote_images 命令；无论成功失败都使图片列表查询（monitorKeys.images）失效以重新拉取
  const upload = useMutation({
    mutationFn: uploadRemoteImages,
    onSettled: () =>
      queryClient.invalidateQueries({ queryKey: monitorKeys.images() }),
  });

  // 删除单张图片的 mutation：对接 delete_remote_image 命令；成功后使图片列表查询失效以重新拉取
  const remove = useMutation({
    mutationFn: deleteRemoteImage,
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: monitorKeys.images() }),
  });

  // 汇总所有相关查询/mutation 的错误，任意一个出错就展示错误提示
  const error =
    settings.error ?? devices.error ?? images.error ?? upload.error ?? remove.error;
  // 图片列表数据，查询未返回数据时兜底为空数组
  const imageList = images.data ?? [];
  // 按类型（全部/jpeg/png/gif）过滤图片列表，并统计各类型数量
  const { category, setCategory, filteredImages, counts } =
    useImageCategoryFilter(imageList);

  // 设备门禁：设备未配置或不可用时，返回统一的拦截提示，阻止渲染正式页面内容
  const deviceGate = useMonitorDeviceGate({
    isPending: monitorPending,
    hasConfiguredDevice,
    hasAvailableDevice,
    featureLabel: t("image.feature"),
  });
  if (deviceGate) return deviceGate;

  return (
    <Stack gap="md">
      {/* 顶部操作区：刷新图片列表按钮、批量上传按钮（及其背后隐藏的文件选择 input） */}
      <Group justify="flex-end" gap="sm">
        <Button
          variant="default"
          leftSection={<LineIcon name="refresh" size={17} />}
          // 手动重新拉取远端图片列表
          onClick={() => images.refetch()}
          loading={images.isFetching}
        >
          {t("common.refresh")}
        </Button>
        <Button
          leftSection={<LineIcon name="upload" size={17} />}
          // 触发隐藏的文件选择 input 弹出系统选择框
          onClick={() => inputRef.current?.click()}
          loading={upload.isPending}
        >
          {t("image.upload")}
        </Button>
        <input
          ref={inputRef}
          hidden
          type="file"
          multiple
          accept=".bmp,.jpg,.jpeg,.gif,.png,.webp,image/bmp,image/jpeg,image/gif,image/png,image/webp"
          // 选中文件后触发上传 mutation，并清空 input 的值以便下次可重复选择同一文件
          onChange={(event) => {
            const files = Array.from(event.currentTarget.files ?? []);
            if (files.length > 0) upload.mutate(files);
            event.currentTarget.value = "";
          }}
        />
      </Group>

      {/* 汇总错误提示 */}
      {error && <Alert color="red">{error.message}</Alert>}
      {/* 上传成功后的提示，展示成功上传的图片数量 */}
      {upload.isSuccess && (
        <Alert color="teal">
          {t("image.uploaded", { count: upload.data.length })}
        </Alert>
      )}

      {/* 三种主状态分支：查询中显示加载态；已有图片数据时显示分类筛选与图片网格；否则显示空态引导 */}
      {images.isPending ? (
        <Center py={80}>
          <Loader />
        </Center>
      ) : images.data?.length ? (
        <>
          {/* 图片数量统计文案与按类型筛选的分段控件 */}
          <Group justify="space-between" align="center">
            <Text size="sm" c="dimmed">
              {category === "all"
                ? t("common.imagesTotal", { count: imageList.length })
                : t("common.imagesFiltered", { visible: filteredImages.length, total: imageList.length })}
            </Text>
            <SegmentedControl
              size="sm"
              value={category}
              onChange={(value) => setCategory(value as typeof category)}
              data={[
                { value: "all", label: t("common.allCount", { count: imageList.length }) },
                { value: "jpeg", label: `JPEG ${counts.jpeg}` },
                { value: "png", label: `PNG ${counts.png}` },
                { value: "gif", label: `GIF ${counts.gif}` },
              ]}
            />
          </Group>
          {/* 按当前筛选类型渲染图片网格；筛选后没有结果时显示“该分类暂无图片”提示 */}
          {filteredImages.length ? (
            <SimpleGrid
              cols={{ base: 1, xs: 2, md: 3, xl: 4 }}
              spacing="md"
            >
              {filteredImages.map((image) => (
                <RemoteImageCard
                  key={image.filename}
                  image={image}
                  onDelete={() => remove.mutate(image.filename)}
                />
              ))}
            </SimpleGrid>
          ) : (
            <Center py={64}>
              <Text c="dimmed">{t("image.emptyCategory")}</Text>
            </Center>
          )}
        </>
      ) : (
        // 远端完全没有图片时的空态引导卡片，提供快捷上传入口
        <Card withBorder className="empty-state">
          <div className="empty-state-icon">
            <LineIcon name="image" size={30} />
          </div>
          <Text fw={650} mt="md">
            {t("image.emptyTitle")}
          </Text>
          <Text c="dimmed" size="sm" maw={360} ta="center" mt={6}>
            {t("image.emptyDescription")}
          </Text>
          <Button
            mt="lg"
            variant="light"
            onClick={() => inputRef.current?.click()}
          >
            {t("image.chooseMultiple")}
          </Button>
        </Card>
      )}
    </Stack>
  );
}

// 单张远端图片卡片的 props：图片数据与删除回调
interface RemoteImageCardProps {
  image: RemoteImage;
  onDelete: () => void;
}

// 单张远端图片卡片：展示图片预览、类型徽标、加载后的实际像素尺寸，以及删除操作菜单
function RemoteImageCard({ image, onDelete }: RemoteImageCardProps) {
  const { t } = useI18n();
  // 图片加载完成后读取到的实际宽高，加载前为 null（显示“读取中…”）
  const [dimensions, setDimensions] = useState<{
    width: number;
    height: number;
  } | null>(null);

  return (
    <Card withBorder padding={0} className="image-card">
      {/* 图片预览区域，加载完成后记录其原始宽高；右上角叠加类型徽标 */}
      <div className="image-preview">
        <img
          src={image.image}
          alt={image.filename}
          onLoad={(event) =>
            setDimensions({
              width: event.currentTarget.naturalWidth,
              height: event.currentTarget.naturalHeight,
            })
          }
        />
        <Badge className="image-type" variant="filled" color="dark" size="xs">
          {image.mimeType.replace("image/", "").toUpperCase()}
        </Badge>
      </div>
      {/* 底部信息栏：显示图片尺寸（或加载中占位文案）与来源说明，右侧提供删除操作菜单 */}
      <Group p="sm" justify="space-between" wrap="nowrap">
        <div className="min-width-zero">
          <Text fw={600} size="sm" truncate>
            {dimensions ? `${dimensions.width} × ${dimensions.height}` : t("image.reading")}
          </Text>
          <Text c="dimmed" size="xs" mt={3}>
            {t("image.remoteCache")}
          </Text>
        </div>
        <Menu shadow="md" position="bottom-end">
          <Menu.Target>
            <Button variant="subtle" color="gray" px={10}>
              ···
            </Button>
          </Menu.Target>
          <Menu.Dropdown>
            {/* 删除当前图片的菜单项 */}
            <Menu.Item
              color="red"
              leftSection={<LineIcon name="trash" size={16} />}
              onClick={onDelete}
            >
              {t("image.delete")}
            </Menu.Item>
          </Menu.Dropdown>
        </Menu>
      </Group>
    </Card>
  );
}
