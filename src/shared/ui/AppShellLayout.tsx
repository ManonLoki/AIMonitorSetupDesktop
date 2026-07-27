// 引入 Mantine 的外壳布局、按钮、汉堡菜单、分组容器、导航链接、文本、提示框、主题色 hook 等组件
import {
  ActionIcon,
  AppShell,
  Burger,
  Group,
  NavLink,
  Text,
  Tooltip,
  useMantineColorScheme,
} from "@mantine/core";
// 引入 Mantine 的开关状态 hook（用于控制移动端侧边栏展开/收起）
import { useDisclosure } from "@mantine/hooks";
// 引入 TanStack Router 的链接组件、路由出口组件、路由状态 hook
import { Link, Outlet, useRouterState } from "@tanstack/react-router";
// 引入 Jotai 的读写 atom hook 与只写 atom hook
import { useAtom, useSetAtom } from "jotai";
// 引入应用图标资源
import appIconUrl from "../../../src-tauri/icons/ai-monitor/128x128.png";
// 引入颜色主题与侧边栏折叠状态的 Jotai atom
import { colorSchemeAtom, sidebarCollapsedAtom } from "../state/ui";
// 引入自定义线性图标组件
import { LineIcon } from "./LineIcon";
// 引入 TanStack Query 的查询 hook
import { useQuery } from "@tanstack/react-query";
// 引入系统概览查询配置（用于展示当前应用版本号）
import { systemOverviewQuery } from "../../features/system/queries/system";
// 引入设备切换菜单组件（侧边栏底部）
import { DeviceSwitchMenu } from "../../features/monitor/components/DeviceSwitchMenu";
// 引入监控连接状态 hook，用于判断当前是否有可用设备
import { useMonitorConnection } from "../../features/monitor/hooks/useMonitorConnection";
import { AnimatedContent } from "./react-bits/AnimatedContent";
import { OnboardingGuide } from "./OnboardingGuide";
// 引入路由路径常量的唯一来源
import { ROUTES } from "../routes";

// 侧边导航菜单项配置：标签、目标路由、图标名、是否需要设备才可点击
const navigation = [
  { label: "工作台", to: ROUTES.home, icon: "dashboard" as const, requiresDevice: false },
  { label: "监控管理", to: ROUTES.aiManagement, icon: "ai" as const, requiresDevice: true },
  { label: "图片管理", to: ROUTES.images, icon: "image" as const, requiresDevice: true },
  { label: "设置", to: ROUTES.settings, icon: "settings" as const, requiresDevice: false },
];

// 应用整体外壳布局：侧边导航栏 + 主内容区（Outlet 渲染当前路由页面）
export function AppShellLayout() {
  // 移动端侧边栏展开/收起状态（opened 为是否展开，toggle/close 为操作方法）
  const [opened, { toggle, close }] = useDisclosure();
  // 从路由状态中读取当前路径名，用于高亮当前激活的导航项
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  // 读取当前 Mantine 颜色方案（light/dark）
  const { colorScheme } = useMantineColorScheme();
  // 获取写入颜色主题 atom 的方法，用于切换主题
  const setColorScheme = useSetAtom(colorSchemeAtom);
  // 侧边栏是否折叠的状态及其更新函数（桌面端窄侧边栏模式）
  const [collapsed, setCollapsed] = useAtom(sidebarCollapsedAtom);
  // 查询系统概览信息（含版本号），用于品牌区展示
  const overview = useQuery(systemOverviewQuery);
  // 获取当前是否存在可用监控设备，用于控制导航项的禁用状态
  const { hasAvailableDevice } = useMonitorConnection();

  return (
    <AppShell
      navbar={{
        // 侧边栏宽度：折叠时 84px，展开时 232px
        width: collapsed ? 84 : 232,
        breakpoint: "sm",
        // 移动端断点以下，根据 opened 状态决定是否折叠侧边栏
        collapsed: { mobile: !opened },
      }}
      padding={{ base: "xs", sm: "md" }}
      className="app-shell"
    >
      <AppShell.Navbar
        p={collapsed ? "xs" : "md"}
        className={collapsed ? "app-navbar collapsed" : "app-navbar"}
      >
        {/* 品牌区：应用图标 + 名称/版本，以及主题切换、侧边栏折叠按钮 */}
        <div className={collapsed ? "app-brand collapsed" : "app-brand"}>
          <Group gap="sm" wrap="nowrap" className="app-brand-identity">
            <img className="brand-mark" src={appIconUrl} alt="" />
            {/* 折叠状态下隐藏文字，仅保留图标 */}
            {!collapsed && (
              <div className="brand-copy">
                <Text fw={700}>AI Monitor</Text>
                <Text size="xs" c="dimmed">
                  {/* 版本号加载完成前展示占位文案 */}
                  {overview.data ? `v${overview.data.version}` : "版本加载中"}
                </Text>
              </div>
            )}
          </Group>
          <Group gap={4} wrap="nowrap" className="app-brand-controls">
            {/* 主题切换按钮：根据当前主题展示提示文案与图标 */}
            <Tooltip
              label={colorScheme === "dark" ? "浅色模式" : "深色模式"}
              position={collapsed ? "right" : "bottom"}
            >
              <ActionIcon
                variant="subtle"
                color="gray"
                size="sm"
                onClick={() =>
                  // 点击时在浅色/深色之间切换主题
                  setColorScheme(colorScheme === "dark" ? "light" : "dark")
                }
                aria-label="切换主题"
              >
                <LineIcon
                  name={colorScheme === "dark" ? "sun" : "moon"}
                  size={16}
                />
              </ActionIcon>
            </Tooltip>
            {/* 侧边栏折叠/展开按钮 */}
            <Tooltip
              label={collapsed ? "展开侧边栏" : "收起侧边栏"}
              position={collapsed ? "right" : "bottom"}
            >
              <ActionIcon
                variant="subtle"
                color="gray"
                size="sm"
                onClick={() => setCollapsed((value) => !value)}
                aria-label="切换侧边栏"
              >
                <LineIcon name="panel" size={16} />
              </ActionIcon>
            </Tooltip>
          </Group>
        </div>
        <nav>
          {/* 遍历导航配置，逐项渲染导航链接 */}
          {navigation.map((item) => {
            // 若该项需要设备且当前无可用设备，则禁用该导航项
            const disabled = item.requiresDevice && !hasAvailableDevice;
            return collapsed ? (
              // 折叠模式：仅展示图标，用 Tooltip 显示完整标签
              <Tooltip key={item.to} label={item.label} position="right">
                <span
                  aria-label={`${item.label}${disabled ? "（无可用设备）" : ""}`}
                  aria-disabled={disabled || undefined}
                  className={
                    `rail-link${pathname === item.to ? " active" : ""}${disabled ? " disabled" : ""}`
                  }
                >
                  {disabled ? (
                    // 禁用状态下只展示图标，不可点击跳转
                    <LineIcon name={item.icon} size={19} />
                  ) : (
                    // 可用状态下包一层 Link，点击后关闭移动端侧边栏并跳转
                    <Link to={item.to} onClick={close} aria-label={item.label}>
                      <LineIcon name={item.icon} size={19} />
                    </Link>
                  )}
                </span>
              </Tooltip>
            ) : (
              // 展开模式：使用 Mantine NavLink 展示图标 + 文字标签
              <NavLink
                key={item.to}
                component={Link}
                to={item.to}
                label={item.label}
                leftSection={<LineIcon name={item.icon} size={19} />}
                active={pathname === item.to}
                onClick={close}
                disabled={disabled}
                className="sidebar-link"
              />
            );
          })}
        </nav>
        {/* 侧边栏底部：设备切换菜单 */}
        <div className="sidebar-footer">
          <DeviceSwitchMenu collapsed={collapsed} />
        </div>
      </AppShell.Navbar>

      <AppShell.Main>
        {/* 移动端专属的汉堡菜单按钮，用于展开/收起侧边栏，桌面端隐藏 */}
        <Burger
          opened={opened}
          onClick={toggle}
          hiddenFrom="sm"
          size="sm"
          aria-label="切换导航"
          className="mobile-nav-trigger"
        />
        <main className="page-container">
          {/* 路径变化时重新挂载 ReactBits 动效，让每个业务页面获得一致的入场过渡 */}
          <AnimatedContent
            key={pathname}
            distance={18}
            duration={0.48}
            ease="power3.out"
            initialOpacity={0.2}
            scale={0.995}
            threshold={0.01}
            className="react-bits-page"
          >
            <Outlet />
          </AnimatedContent>
        </main>
      </AppShell.Main>
      <OnboardingGuide />
    </AppShell>
  );
}
