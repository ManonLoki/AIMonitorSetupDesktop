//! 设备选择、在线列表与快照 revision 的串行化提交。

use std::sync::MutexGuard;

use super::super::{DeviceSnapshotState, discovery::stabilize_discovered_devices};
use super::{MonitorDeviceSnapshot, MonitorService};
use crate::domain::monitor::{
    DiscoveredMonitorDevice, MonitorDeviceRoute, MonitorSettings, validate_device_route,
};

impl MonitorService {
    /// 获取设备快照事务锁。连接读取也复用该锁，保证从 settings 到
    /// `online_devices` 的组合读不会被发现提交或手动切换打断。
    pub(in crate::application::monitor) fn lock_device_snapshot_transaction(
        &self,
    ) -> Result<MutexGuard<'_, DeviceSnapshotState>, String> {
        self.device_snapshot_state
            .lock()
            .map_err(|_| "设备快照事务锁已损坏".to_owned())
    }

    /// 为一次完整发现分配启动世代。网络探测在事务锁外运行；提交时
    /// 若已有更新世代启动，该较旧结果会被丢弃。
    pub(super) fn begin_online_device_refresh(&self) -> Result<u64, String> {
        let mut state = self.lock_device_snapshot_transaction()?;
        state.latest_refresh_generation = state.latest_refresh_generation.saturating_add(1);
        Ok(state.latest_refresh_generation)
    }

    /// 将一轮原始发现结果转换成稳定在线快照。去抖、自动选择、
    /// 内存替换、revision 分配与 DTO 构造全部位于同一串行临界区。
    pub(super) fn update_online_devices(
        &self,
        refresh_generation: u64,
        devices: Vec<DiscoveredMonitorDevice>,
    ) -> Result<(MonitorDeviceSnapshot, bool), String> {
        let mut state = self.lock_device_snapshot_transaction()?;
        if refresh_generation != state.latest_refresh_generation {
            return Ok((self.current_device_snapshot_locked(state.revision)?, false));
        }
        let devices = {
            let mut missed_scans = self
                .discovery_missed_scans
                .lock()
                .map_err(|_| "设备发现去抖锁已损坏".to_owned())?;
            let previous = self
                .online_devices
                .read()
                .map_err(|_| "在线设备读取锁已损坏".to_owned())?
                .clone();
            stabilize_discovered_devices(&previous, devices, &mut missed_scans)
        };

        let selection_changed = self.select_first_available_device_if_needed_locked(&devices)?;
        let devices_changed = self.replace_online_devices_locked(&devices)?;
        let changed = selection_changed || devices_changed;
        if changed {
            self.advance_device_snapshot_revision_locked(&mut state);
        }
        let snapshot = MonitorDeviceSnapshot::from_parts(state.revision, self.settings()?, devices);
        Ok((snapshot, changed))
    }

    fn select_first_available_device_if_needed_locked(
        &self,
        devices: &[DiscoveredMonitorDevice],
    ) -> Result<bool, String> {
        let Some(next) = devices.first() else {
            return Ok(false);
        };
        let settings = self.settings()?;
        if devices.iter().any(|device| device.id == settings.device_id) {
            return Ok(false);
        }
        self.select_device_locked(next)
            .map(|(_, selection_changed)| selection_changed)
    }

    fn replace_online_devices_locked(
        &self,
        devices: &[DiscoveredMonitorDevice],
    ) -> Result<bool, String> {
        let mut current = self
            .online_devices
            .write()
            .map_err(|_| "在线设备写入锁已损坏".to_owned())?;
        if current.as_slice() == devices {
            return Ok(false);
        }
        *current = devices.to_vec();
        Ok(true)
    }

    /// 选中设备并持久化最新路由。直接业务调用也必须经过快照事务锁，
    /// 避免绕过 `select_device_snapshot` 时与后台发现交错写入。
    #[cfg(test)]
    pub fn select_device(
        &self,
        device: &DiscoveredMonitorDevice,
    ) -> Result<MonitorSettings, String> {
        let mut state = self.lock_device_snapshot_transaction()?;
        let (settings, changed) = self.select_device_locked(device)?;
        if changed {
            self.advance_device_snapshot_revision_locked(&mut state);
        }
        Ok(settings)
    }

    /// 选择设备并返回与当前在线列表同一串行版本的原子快照。
    pub fn select_device_snapshot(
        &self,
        device: &DiscoveredMonitorDevice,
    ) -> Result<MonitorDeviceSnapshot, String> {
        let mut state = self.lock_device_snapshot_transaction()?;
        let devices = self
            .online_devices
            .read()
            .map_err(|_| "在线设备读取锁已损坏".to_owned())?
            .clone();
        // 输入 DTO 可能来自旧的前端缓存；只取其稳定 ID，名称和地址必须
        // 使用同一事务中的最新在线记录。已离线设备不允许被迟到选择复活。
        let online_device = devices
            .iter()
            .find(|online| online.id == device.id)
            .cloned()
            .ok_or_else(|| "目标 AIMonitor 设备已离线，请刷新设备列表".to_owned())?;
        let (settings, changed) = self.select_device_locked(&online_device)?;
        if changed {
            self.advance_device_snapshot_revision_locked(&mut state);
        }
        Ok(MonitorDeviceSnapshot::from_parts(
            state.revision,
            settings,
            devices,
        ))
    }

    fn current_device_snapshot_locked(
        &self,
        revision: u64,
    ) -> Result<MonitorDeviceSnapshot, String> {
        let devices = self
            .online_devices
            .read()
            .map_err(|_| "在线设备读取锁已损坏".to_owned())?
            .clone();
        Ok(MonitorDeviceSnapshot::from_parts(
            revision,
            self.settings()?,
            devices,
        ))
    }

    fn advance_device_snapshot_revision_locked(&self, state: &mut DeviceSnapshotState) {
        state.revision = state.revision.saturating_add(1);
        self.hook_device_snapshot_revision
            .send_replace(state.revision);
    }

    /// 调用方必须已持有设备快照事务锁。设备选择和已保存路由都未变时
    /// 不写盘，也不推进 revision。
    fn select_device_locked(
        &self,
        device: &DiscoveredMonitorDevice,
    ) -> Result<(MonitorSettings, bool), String> {
        let route = validate_device_route(device)?;
        let mut data = self
            .data
            .write()
            .map_err(|_| "配置写入锁已损坏".to_owned())?;
        let settings_unchanged = data.settings.base_url == route.base_url
            && data.settings.device_id == route.device_id
            && data.settings.device_name == route.device_name;
        let route_unchanged = data
            .devices
            .iter()
            .any(|existing| same_route(existing, &route));
        if settings_unchanged && route_unchanged {
            return Ok((data.settings.clone(), false));
        }

        let mut next_data = data.clone();
        next_data.settings.base_url.clone_from(&route.base_url);
        next_data.settings.device_id.clone_from(&route.device_id);
        next_data
            .settings
            .device_name
            .clone_from(&route.device_name);
        next_data
            .devices
            .retain(|existing| existing.device_id != route.device_id);
        next_data.devices.push(route);
        next_data
            .devices
            .sort_by(|left, right| left.device_id.cmp(&right.device_id));
        self.persist(&next_data)?;
        *data = next_data;
        Ok((data.settings.clone(), true))
    }
}

fn same_route(left: &MonitorDeviceRoute, right: &MonitorDeviceRoute) -> bool {
    left.device_id == right.device_id
        && left.device_name == right.device_name
        && left.base_url == right.base_url
}
