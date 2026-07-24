import {
  ActionIcon,
  Group,
  Popover,
  SimpleGrid,
  Text,
  Tooltip,
  UnstyledButton,
} from "@mantine/core";
import { useId, useState } from "react";
import type { RemoteImage } from "../api/monitor";
import { LineIcon } from "../../../shared/ui/LineIcon";

interface ImagePickerProps {
  images: RemoteImage[];
  value: string;
  disabled?: boolean;
  onChange: (value: string) => void;
}

export function ImagePicker({
  images,
  value,
  disabled,
  onChange,
}: ImagePickerProps) {
  const [opened, setOpened] = useState(false);
  const labelId = useId();
  const selectedImage = images.find((image) => image.filename === value);

  return (
    <div>
      <Group justify="space-between" mb={7}>
        <Text component="span" size="sm" fw={500} id={labelId}>
          展示图片 <span className="required-mark">*</span>
        </Text>
        {selectedImage && (
          <Tooltip label="清除选择">
            <ActionIcon
              variant="subtle"
              color="gray"
              size="sm"
              aria-label="清除已选图片"
              onClick={() => onChange("")}
            >
              <LineIcon name="close" size={15} />
            </ActionIcon>
          </Tooltip>
        )}
      </Group>

      <Popover
        opened={opened}
        onChange={setOpened}
        position="bottom-start"
        width={392}
        shadow="lg"
        withinPortal
      >
        <Popover.Target>
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
                  更换图片
                </span>
              </>
            ) : (
              <span className="image-picker-empty">
                <LineIcon name="image" size={24} />
                {disabled ? "正在加载图片" : "点击选择图片"}
              </span>
            )}
          </UnstyledButton>
        </Popover.Target>

        <Popover.Dropdown p="sm">
          <Group justify="space-between" mb="sm">
            <div>
              <Text size="sm" fw={650}>
                选择展示图片
              </Text>
              <Text size="xs" c="dimmed">
                共 {images.length} 张
              </Text>
            </div>
            <ActionIcon
              variant="subtle"
              color="gray"
              aria-label="关闭图片选择"
              onClick={() => setOpened(false)}
            >
              <LineIcon name="close" size={17} />
            </ActionIcon>
          </Group>

          <div className="image-picker-options" role="listbox">
            <SimpleGrid cols={4} spacing="xs">
              {images.map((image) => (
                <Tooltip key={image.filename} label={image.filename}>
                  <UnstyledButton
                    type="button"
                    role="option"
                    aria-selected={image.filename === value}
                    aria-label={`选择图片 ${image.filename}`}
                    className="image-picker-option"
                    data-selected={image.filename === value || undefined}
                    onClick={() => {
                      onChange(image.filename);
                      setOpened(false);
                    }}
                  >
                    <img src={image.image} alt="" />
                    {image.filename === value && (
                      <span className="image-picker-check">
                        <LineIcon name="check" size={14} />
                      </span>
                    )}
                  </UnstyledButton>
                </Tooltip>
              ))}
            </SimpleGrid>
          </div>
        </Popover.Dropdown>
      </Popover>
    </div>
  );
}
