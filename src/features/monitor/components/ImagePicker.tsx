import {
  ActionIcon,
  Button,
  Group,
  Popover,
  SegmentedControl,
  SimpleGrid,
  Text,
  Tooltip,
  UnstyledButton,
} from "@mantine/core";
import { useId, useRef, useState } from "react";
import type { RemoteImage } from "../api/monitor";
import { useImageCategoryFilter } from "../hooks/useImageCategoryFilter";
import { LineIcon } from "../../../shared/ui/LineIcon";
import { useI18n } from "../../../shared/i18n";

// images 为可选图片列表，value 为当前已选图片的文件名，onChange 在选择/清除时回调
interface ImagePickerProps {
  images: RemoteImage[];
  value: string;
  disabled?: boolean;
  uploading?: boolean;
  onChange: (value: string) => void;
  onUpload: (file: File) => void;
}

export function ImagePicker({
  images,
  value,
  disabled,
  uploading,
  onChange,
  onUpload,
}: ImagePickerProps) {
  const { t } = useI18n();
  // 控制弹出层（Popover）的展开/收起状态
  const [opened, setOpened] = useState(false);
  // 单图上传入口使用隐藏 input；与图片管理页的批量上传相互独立。
  const uploadInputRef = useRef<HTMLInputElement>(null);
  // 图片分类筛选 hook：提供当前分类、切换分类方法、按分类过滤后的图片、各分类数量
  const { category, setCategory, filteredImages, counts } =
    useImageCategoryFilter(images);
  // 生成唯一 id，用于关联标签文本与触发按钮的无障碍属性
  const labelId = useId();
  // 根据 value 在全部图片中查找当前已选中的图片对象
  const selectedImage = images.find((image) => image.filename === value);

  return (
    <div>
      {/* 字段标题行：展示"展示图片"标签，若已选图片则显示清除按钮 */}
      <Group justify="space-between" mb={7}>
        <Text component="span" size="sm" fw={500} id={labelId}>
          {t("image.field")} <span className="required-mark">*</span>
        </Text>
        {selectedImage && (
          <Tooltip label={t("image.clear")}>
            <ActionIcon
              variant="subtle"
              color="gray"
              size="sm"
              aria-label={t("image.clearAria")}
              onClick={() => onChange("")}
            >
              <LineIcon name="close" size={15} />
            </ActionIcon>
          </Tooltip>
        )}
      </Group>

      {/* 图片选择弹出层：点击触发按钮展开，内部按分类展示图片九宫格供选择 */}
      <Popover
        opened={opened}
        onChange={setOpened}
        position="bottom-start"
        width={392}
        shadow="lg"
        withinPortal
      >
        <Popover.Target>
          {/* 触发按钮：已选图片时展示图片预览与"更换图片"遮罩，未选时展示空态提示 */}
          <UnstyledButton
            type="button"
            className="image-picker-trigger"
            data-empty={!selectedImage || undefined}
            disabled={disabled}
            aria-labelledby={labelId}
            aria-haspopup="listbox"
            aria-expanded={opened}
            onClick={() => setOpened((current) => !current)}
          >
            {selectedImage ? (
              <>
                <img src={selectedImage.image} alt={selectedImage.filename} />
                <span className="image-picker-overlay">
                  <LineIcon name="image" size={17} />
                  {t("image.change")}
                </span>
              </>
            ) : (
              <span className="image-picker-empty">
                <LineIcon name="image" size={24} />
                {disabled ? t("image.loading") : t("image.clickChoose")}
              </span>
            )}
          </UnstyledButton>
        </Popover.Target>

        <Popover.Dropdown p="sm">
          {/* 弹出层头部：标题、按当前分类展示的图片计数、关闭按钮 */}
          <Group justify="space-between" mb="sm">
            <div>
              <Text size="sm" fw={650}>
                {t("image.chooseTitle")}
              </Text>
              <Text size="xs" c="dimmed">
                {category === "all"
                  ? t("common.imagesTotal", { count: images.length })
                  : t("common.imagesFiltered", { visible: filteredImages.length, total: images.length })}
              </Text>
            </div>
            <Group gap="xs" wrap="nowrap">
              <Button
                size="xs"
                variant="light"
                leftSection={<LineIcon name="upload" size={15} />}
                aria-label={t("image.uploadSingleAria")}
                loading={uploading}
                disabled={disabled}
                onClick={() => uploadInputRef.current?.click()}
              >
                {uploading ? t("image.uploading") : t("image.uploadSingle")}
              </Button>
              <input
                ref={uploadInputRef}
                hidden
                type="file"
                accept=".bmp,.jpg,.jpeg,.gif,.png,.webp,image/bmp,image/jpeg,image/gif,image/png,image/webp"
                disabled={disabled}
                onChange={(event) => {
                  const file = event.currentTarget.files?.[0];
                  if (file) onUpload(file);
                  event.currentTarget.value = "";
                }}
              />
              <ActionIcon
                variant="subtle"
                color="gray"
                aria-label={t("image.closeAria")}
                onClick={() => setOpened(false)}
              >
                <LineIcon name="close" size={17} />
              </ActionIcon>
            </Group>
          </Group>

          {/* 分类切换控件：全部/JPEG/PNG/GIF，各项附带对应数量 */}
          <SegmentedControl
            fullWidth
            size="xs"
            mb="sm"
            value={category}
            onChange={(nextCategory) =>
              setCategory(nextCategory as typeof category)
            }
            data={[
              { value: "all", label: t("common.allCount", { count: images.length }) },
              { value: "jpeg", label: `JPEG ${counts.jpeg}` },
              { value: "png", label: `PNG ${counts.png}` },
              { value: "gif", label: `GIF ${counts.gif}` },
            ]}
          />

          {/* 图片九宫格区域：有图片时按分类渲染四列网格，无图片时展示空态文案 */}
          <div className="image-picker-options" role="listbox">
            {filteredImages.length ? (
              <SimpleGrid cols={4} spacing="xs">
                {filteredImages.map((image) => (
                  <Tooltip key={image.filename} label={image.filename}>
                    <UnstyledButton
                      type="button"
                      role="option"
                      aria-selected={image.filename === value}
                      aria-label={t("image.selectAria", { filename: image.filename })}
                      className="image-picker-option"
                      data-selected={image.filename === value || undefined}
                      onClick={() => {
                        // 点击图片后回填选中的文件名并关闭弹出层
                        onChange(image.filename);
                        setOpened(false);
                      }}
                    >
                      <img src={image.image} alt="" />
                      {image.filename === value && (
                        // 已选中的图片右上角展示对勾图标
                        <span className="image-picker-check">
                          <LineIcon name="check" size={14} />
                        </span>
                      )}
                    </UnstyledButton>
                  </Tooltip>
                ))}
              </SimpleGrid>
            ) : (
              <Text c="dimmed" size="sm" ta="center" py="xl">
                {t("image.emptyCategory")}
              </Text>
            )}
          </div>
        </Popover.Dropdown>
      </Popover>
    </div>
  );
}
