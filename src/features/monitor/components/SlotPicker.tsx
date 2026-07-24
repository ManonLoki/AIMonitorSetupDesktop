import { Badge, Group, Text, UnstyledButton } from "@mantine/core";

interface SlotPickerProps {
  value: number;
  onChange: (value: number) => void;
}

const slots = Array.from({ length: 25 }, (_, index) => index + 1);

export function SlotPicker({ value, onChange }: SlotPickerProps) {
  return (
    <div className="slot-picker">
      <Group justify="space-between" align="flex-start" mb="sm">
        <div>
          <Text fw={650}>显示位置</Text>
          <Text size="sm" c="dimmed" mt={3}>
            点击设备上的对应格子
          </Text>
        </div>
        <Badge variant="light" color="violet" size="lg">
          位置 {value}
        </Badge>
      </Group>

      <div className="slot-grid" role="group" aria-label="选择显示位置">
        {slots.map((slot) => {
          const row = Math.ceil(slot / 5);
          const column = ((slot - 1) % 5) + 1;

          return (
            <UnstyledButton
              key={slot}
              type="button"
              className="slot-cell"
              data-selected={slot === value || undefined}
              aria-pressed={slot === value}
              aria-label={`位置 ${slot}，第 ${row} 行第 ${column} 列`}
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
