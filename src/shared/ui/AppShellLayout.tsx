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
import { useDisclosure } from "@mantine/hooks";
import { Link, Outlet, useRouterState } from "@tanstack/react-router";
import { useAtom, useSetAtom } from "jotai";
import appIconUrl from "../../../src-tauri/icons/ai-monitor/128x128.png";
import { colorSchemeAtom, sidebarCollapsedAtom } from "../state/ui";
import { LineIcon } from "./LineIcon";
import { useQuery } from "@tanstack/react-query";
import { systemOverviewQuery } from "../../features/system/queries/system";
import { DeviceSwitchMenu } from "../../features/monitor/components/DeviceSwitchMenu";

const navigation = [
  { label: "工作台", to: "/" as const, icon: "dashboard" as const },
  { label: "AI 管理", to: "/ai-management" as const, icon: "ai" as const },
  { label: "图片管理", to: "/images" as const, icon: "image" as const },
  { label: "设置", to: "/settings" as const, icon: "settings" as const },
];

export function AppShellLayout() {
  const [opened, { toggle, close }] = useDisclosure();
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  const { colorScheme } = useMantineColorScheme();
  const setColorScheme = useSetAtom(colorSchemeAtom);
  const [collapsed, setCollapsed] = useAtom(sidebarCollapsedAtom);
  const overview = useQuery(systemOverviewQuery);

  return (
    <AppShell
      navbar={{
        width: collapsed ? 84 : 232,
        breakpoint: "sm",
        collapsed: { mobile: !opened },
      }}
      padding={{ base: "xs", sm: "md" }}
      className="app-shell"
    >
      <AppShell.Navbar
        p={collapsed ? "xs" : "md"}
        className={collapsed ? "app-navbar collapsed" : "app-navbar"}
      >
        <div className={collapsed ? "app-brand collapsed" : "app-brand"}>
          <Group gap="sm" wrap="nowrap" className="app-brand-identity">
            <img className="brand-mark" src={appIconUrl} alt="" />
            {!collapsed && (
              <div className="brand-copy">
                <Text fw={700}>AI Monitor</Text>
                <Text size="xs" c="dimmed">
                  {overview.data ? `v${overview.data.version}` : "版本加载中"}
                </Text>
              </div>
            )}
          </Group>
          <Group gap={4} wrap="nowrap" className="app-brand-controls">
            <Tooltip
              label={colorScheme === "dark" ? "浅色模式" : "深色模式"}
              position={collapsed ? "right" : "bottom"}
            >
              <ActionIcon
                variant="subtle"
                color="gray"
                size="sm"
                onClick={() =>
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
          {navigation.map((item) =>
            collapsed ? (
              <Tooltip key={item.to} label={item.label} position="right">
                <Link
                  to={item.to}
                  onClick={close}
                  aria-label={item.label}
                  className={
                    pathname === item.to ? "rail-link active" : "rail-link"
                  }
                >
                  <LineIcon name={item.icon} size={19} />
                </Link>
              </Tooltip>
            ) : (
              <NavLink
                key={item.to}
                component={Link}
                to={item.to}
                label={item.label}
                leftSection={<LineIcon name={item.icon} size={19} />}
                active={pathname === item.to}
                onClick={close}
                className="sidebar-link"
              />
            ),
          )}
        </nav>
        <div className="sidebar-footer">
          <DeviceSwitchMenu collapsed={collapsed} />
        </div>
      </AppShell.Navbar>

      <AppShell.Main>
        <Burger
          opened={opened}
          onClick={toggle}
          hiddenFrom="sm"
          size="sm"
          aria-label="切换导航"
          className="mobile-nav-trigger"
        />
        <main className="page-container">
          <Outlet />
        </main>
      </AppShell.Main>
    </AppShell>
  );
}
