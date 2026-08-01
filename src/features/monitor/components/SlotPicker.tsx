import { Badge, Group, Text, UnstyledButton } from "@mantine/core";
import { useI18n } from "../../../shared/i18n";

interface SlotPickerProps {
  value: number;
  min: number;
  max: number;
  onChange: (value: number) => void;
}

// 设备展示位置网格的列数；列数只决定呈现，槽位范围由 Rust capability 提供。
const GRID_COLUMNS = 5;
export function SlotPicker({ value, min, max, onChange }: SlotPickerProps) {
  const { t } = useI18n();
  const slots = Array.from(
    { length: Math.max(0, max - min + 1) },
    (_, index) => min + index,
  );
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
        {slots.map((slot, index) => {
          // 根据编号计算该格子在网格中的行号和列号，用于无障碍描述
          const row = Math.floor(index / GRID_COLUMNS) + 1;
          const column = (index % GRID_COLUMNS) + 1;

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
