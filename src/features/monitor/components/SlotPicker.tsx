import { Badge, Group, Text, UnstyledButton } from "@mantine/core";
import { useI18n } from "../../../shared/i18n";

interface SlotPickerProps {
  value: number;
  onChange: (value: number) => void;
}

// 设备展示位置网格的列数，与 AiProfile.slot 的取值范围（1-25）配套。
const GRID_COLUMNS = 5;
const TOTAL_SLOTS = 25;

// 生成 1~TOTAL_SLOTS 的位置编号数组，对应设备上的网格布局
const slots = Array.from({ length: TOTAL_SLOTS }, (_, index) => index + 1);

export function SlotPicker({ value, onChange }: SlotPickerProps) {
  const { t } = useI18n();
  return (
    <div className="slot-picker">
      {/* 标题区：说明用途，并用徽标展示当前已选中的位置编号 */}
      <Group justify="space-between" align="flex-start" mb="sm">
        <div>
          <Text fw={650}>{t("slot.title")}</Text>
          <Text size="sm" c="dimmed" mt={3}>
            {t("slot.description")}
          </Text>
        </div>
        <Badge variant="light" color="violet" size="lg">
          {t("slot.position", { slot: value })}
        </Badge>
      </Group>

      {/* 5x5 位置网格，逐格渲染可点击的位置按钮 */}
      <div className="slot-grid" role="group" aria-label={t("slot.groupAria")}>
        {slots.map((slot) => {
          // 根据编号计算该格子在网格中的行号和列号，用于无障碍描述
          const row = Math.ceil(slot / GRID_COLUMNS);
          const column = ((slot - 1) % GRID_COLUMNS) + 1;

          return (
            <UnstyledButton
              key={slot}
              type="button"
              className="slot-cell"
              data-selected={slot === value || undefined}
              aria-pressed={slot === value}
              aria-label={t("slot.cellAria", { slot, row, column })}
              onClick={() => onChange(slot)}
            >
              {slot}
            </UnstyledButton>
          );
        })}
      </div>
    </div>
  );
}
