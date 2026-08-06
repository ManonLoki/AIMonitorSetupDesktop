// 通用线性图标组件：将本地图标名映射到 @tabler/icons-react 的实际图标组件，
// 对外保留 name/size 的调用方式，避免调用方逐个改写。
import type { Icon, IconProps } from "@tabler/icons-react";
import {
  IconChevronDown,
  IconCompass,
  IconLayoutDashboard,
  IconLayoutSidebar,
  IconMoon,
  IconPencil,
  IconPhoto,
  IconRefresh,
  IconRobot,
  IconServer,
  IconSettings,
  IconSquareCheck,
  IconSun,
  IconTrash,
  IconUpload,
  IconX,
} from "@tabler/icons-react";

type IconName =
  | "ai"
  | "dashboard"
  | "image"
  | "settings"
  | "sun"
  | "moon"
  | "refresh"
  | "trash"
  | "edit"
  | "upload"
  | "check"
  | "close"
  | "server"
  | "chevronDown"
  | "panel"
  | "guide";

const icons: Record<IconName, Icon> = {
  ai: IconRobot,
  dashboard: IconLayoutDashboard,
  image: IconPhoto,
  settings: IconSettings,
  sun: IconSun,
  moon: IconMoon,
  refresh: IconRefresh,
  trash: IconTrash,
  edit: IconPencil,
  upload: IconUpload,
  check: IconSquareCheck,
  close: IconX,
  server: IconServer,
  chevronDown: IconChevronDown,
  panel: IconLayoutSidebar,
  guide: IconCompass,
};

export function LineIcon({
  name,
  size = 20,
  ...props
}: Omit<IconProps, "size"> & { name: IconName; size?: number }) {
  const Icon = icons[name];
  return <Icon size={size} stroke={1.8} aria-hidden="true" {...props} />;
}
