// 引入 React 的 ReactNode 类型（图标内部 SVG 片段）与 SVGProps 类型（透传给 svg 标签的属性）
import type { ReactNode, SVGProps } from "react";

// 支持的图标名称枚举，作为 LineIcon 组件 name 属性的合法取值
type IconName =
  | "ai"
  | "dashboard"
  | "image"
  | "settings"
  | "sun"
  | "moon"
  | "plus"
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

// 图标名到具体 SVG 路径片段的映射表，每个图标由若干 <path>/<rect>/<circle> 组成
const paths: Record<IconName, ReactNode> = {
  // “AI”图标：一个带圆角的方框加几个点/线，象征 AI 芯片
  ai: (
    <>
      <rect x="4" y="6" width="16" height="14" rx="3" />
      <path d="M9 11h.01M15 11h.01M9 16h6M12 6V3M8 3h8" />
    </>
  ),
  // “仪表盘”图标：四个小方块组成的九宫格样式
  dashboard: (
    <>
      <rect x="3" y="3" width="7" height="7" rx="1.5" />
      <rect x="14" y="3" width="7" height="7" rx="1.5" />
      <rect x="3" y="14" width="7" height="7" rx="1.5" />
      <rect x="14" y="14" width="7" height="7" rx="1.5" />
    </>
  ),
  // “图片”图标：相框 + 圆形（太阳/头像）+ 山形折线
  image: (
    <>
      <rect x="3" y="4" width="18" height="16" rx="3" />
      <circle cx="9" cy="10" r="2" />
      <path d="m4 17 4-4 3 3 2-2 6 6" />
    </>
  ),
  // “设置”图标：齿轮形状（中心圆 + 外围齿状路径）
  settings: (
    <>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.7 1.7 0 0 0 .34 1.88l.06.06-2.83 2.83-.06-.06a1.7 1.7 0 0 0-1.88-.34 1.7 1.7 0 0 0-1.03 1.56V21h-4v-.08A1.7 1.7 0 0 0 8.94 19.4a1.7 1.7 0 0 0-1.88.34l-.06.06-2.83-2.83.06-.06A1.7 1.7 0 0 0 4.57 15 1.7 1.7 0 0 0 3 14H3v-4h.08A1.7 1.7 0 0 0 4.6 8.94a1.7 1.7 0 0 0-.34-1.88L4.2 7l2.83-2.83.06.06A1.7 1.7 0 0 0 9 4.57 1.7 1.7 0 0 0 10 3V3h4v.08A1.7 1.7 0 0 0 15.06 4.6a1.7 1.7 0 0 0 1.88-.34L17 4.2 19.83 7l-.06.06A1.7 1.7 0 0 0 19.43 9 1.7 1.7 0 0 0 21 10h.08v4H21a1.7 1.7 0 0 0-1.6 1Z" />
    </>
  ),
  // “太阳”图标：切换到浅色模式的按钮图标
  sun: <path d="M12 3V1M12 23v-2M4.22 4.22 2.8 2.8M21.2 21.2l-1.42-1.42M3 12H1M23 12h-2M4.22 19.78 2.8 21.2M21.2 2.8l-1.42 1.42M17 12a5 5 0 1 1-10 0 5 5 0 0 1 10 0Z" />,
  // “月亮”图标：切换到深色模式的按钮图标
  moon: <path d="M20.5 15.2A9 9 0 0 1 8.8 3.5 9 9 0 1 0 20.5 15.2Z" />,
  // “加号”图标：新增操作
  plus: <path d="M12 5v14M5 12h14" />,
  // “刷新”图标：重新加载/刷新数据
  refresh: <path d="M20 7v5h-5M4 17v-5h5M6.1 8A7 7 0 0 1 18.5 6L20 8M4 16l1.5 2A7 7 0 0 0 18 16" />,
  // “删除”图标：垃圾桶样式
  trash: <path d="M4 7h16M9 7V4h6v3M7 7l1 14h8l1-14M10 11v6M14 11v6" />,
  // “编辑”图标：铅笔样式
  edit: <path d="m4 20 4.5-1 10-10a2.1 2.1 0 0 0-3-3l-10 10L4 20ZM14 7l3 3" />,
  // “上传”图标：向上箭头 + 底部横线
  upload: <path d="M12 16V4M7 9l5-5 5 5M5 20h14" />,
  // “勾选”图标：表示确认/成功
  check: <path d="m5 12 4 4L19 6" />,
  // “关闭”图标：叉号
  close: <path d="M6 6l12 12M18 6 6 18" />,
  // “服务器”图标：两层机架样式
  server: <path d="M4 5h16v6H4V5ZM4 13h16v6H4v-6ZM8 8h.01M8 16h.01" />,
  // “向下箭头”图标：常用于下拉指示
  chevronDown: <path d="m6 9 6 6 6-6" />,
  // “面板”图标：带竖分割线的矩形，用于侧边栏折叠按钮
  panel: (
    <>
      <rect x="3" y="4" width="18" height="16" rx="3" />
      <path d="M9 4v16" />
    </>
  ),
  // “引导”图标：指南针轮廓，用于新手引导入口。
  guide: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="m15.5 8.5-2.1 4.9-4.9 2.1 2.1-4.9 4.9-2.1Z" />
    </>
  ),
};

// 通用线性图标组件：根据 name 选取对应的 SVG 路径片段渲染，
// size 控制宽高，其余 SVG 属性透传给最外层 <svg> 标签
export function LineIcon({
  name,
  size = 20,
  ...props
}: SVGProps<SVGSVGElement> & { name: IconName; size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      {...props}
    >
      {/* 根据传入的图标名，渲染对应的路径片段 */}
      {paths[name]}
    </svg>
  );
}
