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
import { useSetAtom } from "jotai";
import appIconUrl from "../../../src-tauri/icons/ai-monitor/128x128.png";
import { colorSchemeAtom } from "../state/ui";
import { LineIcon } from "./LineIcon";
import { useQuery } from "@tanstack/react-query";
import { systemOverviewQuery } from "../../features/system/queries/system";

const navigation = [
  { label: "AI 管理", to: "/" as const, icon: "ai" as const },
  { label: "图片管理", to: "/images" as const, icon: "image" as const },
  { label: "设置", to: "/settings" as const, icon: "settings" as const },
];

export function AppShellLayout() {
  const [opened, { toggle, close }] = useDisclosure();
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  const { colorScheme } = useMantineColorScheme();
  const setColorScheme = useSetAtom(colorSchemeAtom);
  const overview = useQuery(systemOverviewQuery);

  return (
    <AppShell
      navbar={{
        width: 232,
        breakpoint: "sm",
        collapsed: { mobile: !opened },
      }}
      padding={{ base: "md", sm: 32 }}
      className="app-shell"
    >
      <AppShell.Navbar p="md" className="app-navbar">
        <Group justify="space-between" mb={28} px={4}>
          <Group gap="sm">
            <img className="brand-mark" src={appIconUrl} alt="" />
            <div className="brand-copy">
              <Text fw={700}>AI Monitor</Text>
              <Text size="xs" c="dimmed">
                {overview.data ? `v${overview.data.version}` : "版本加载中"}
              </Text>
            </div>
          </Group>
        </Group>
        <nav>
          {navigation.map((item) => (
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
          ))}
        </nav>
        <div className="sidebar-footer">
          <div>
            <Group gap="sm">
              <div className="status-dot" />
              <Text size="sm" fw={600}>
                本地控制台
              </Text>
            </Group>
          </div>
          <Tooltip label={colorScheme === "dark" ? "浅色模式" : "深色模式"}>
            <ActionIcon
              variant="subtle"
              color="gray"
              onClick={() =>
                setColorScheme(colorScheme === "dark" ? "light" : "dark")
              }
              aria-label="切换主题"
            >
              <LineIcon
                name={colorScheme === "dark" ? "sun" : "moon"}
                size={18}
              />
            </ActionIcon>
          </Tooltip>
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
