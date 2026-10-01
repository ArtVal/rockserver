import type { BrowserAccount, BrowserDevice } from "../api";

export type JustConnected = Pick<BrowserDevice, "device_display_name" | "device_type">;

export const deviceProductName = (deviceType: string) =>
  deviceType.toLowerCase().includes("mobile") ? "RockMobile" : "RockCast";

export const deviceName = (device: Pick<BrowserDevice, "device_type" | "device_display_name">) => {
  const product = deviceProductName(device.device_type);
  return device.device_display_name.startsWith(`${product} — `)
    ? device.device_display_name
    : `${product} — ${device.device_display_name}`;
};

export const formatDate = (value: string) =>
  new Intl.DateTimeFormat("ru-RU", { dateStyle: "medium", timeStyle: "short" }).format(new Date(value));

/**
 * Hardware HUD component rendering connected RockCast and RockMobile client devices,
 * device pairing status, rename/revoke controls, and device security guidance.
 */
export function HardwareHud({
  account,
  deviceBusy,
  justConnected,
  onRename,
  onRevoke,
}: {
  account: BrowserAccount;
  deviceBusy: string;
  justConnected?: JustConnected;
  onRename: (device: BrowserDevice) => Promise<void>;
  onRevoke: (device: BrowserDevice) => Promise<void>;
}) {
  const limitReached = account.devices.length >= account.device_limit;

  return (
    <div class="hardware-hud space-y-6">
      <section class="deck-panel hardware-deck">
        <div class="panel-header-row">
          <div class="hardware-title-wrap">
            <span class="hud-status-dot glow-cyan" />
            <h2>Подключённые устройства ({account.devices.length})</h2>
          </div>
          <span class="device-limit-pill font-mono-code">
            {account.devices.length} / {account.device_limit} ЛИМИТ
          </span>
        </div>

        {limitReached && (
          <p class="limit-alert" role="status">
            ⚠ Лимит устройств достигнут ({account.device_limit}). Сначала отключите старое устройство.
          </p>
        )}

        {account.devices.length === 0 ? (
          <p class="empty-devices-text">
            Подключённых устройств пока нет. Откройте RockMobile или RockCast и начните подключение там.
          </p>
        ) : (
          <ul class="devices" aria-label="Список подключённых устройств">
            {account.devices.map((device) => {
              const fresh =
                justConnected?.device_display_name === device.device_display_name &&
                justConnected.device_type === device.device_type;
              const busy = deviceBusy === device.device_id;
              const isActive = device.session_status === "active";

              return (
                <li key={device.device_id} class="device-card-item">
                  {fresh && (
                    <p class="fresh" role="status">
                      ✓ Только что подключено
                    </p>
                  )}
                  <div class="device-meta-wrap">
                    <strong class="device-name-text">{deviceName(device)}</strong>
                    <p class="device-session-info">
                      <span class={`status-circle ${isActive ? "active" : "inactive"}`} />
                      <span>{isActive ? "● Сессия активна" : "○ Нет активной сессии"}</span>
                      <span> · Подключено {formatDate(device.connected_at)}</span>
                      {device.last_seen_at && (
                        <span> · Активность {formatDate(device.last_seen_at)}</span>
                      )}
                    </p>
                  </div>
                  <div class="device-actions">
                    <button
                      type="button"
                      class="secondary"
                      onClick={() => onRename(device)}
                      disabled={busy}
                    >
                      {busy ? "Обновляем…" : "Переименовать"}
                    </button>
                    <button
                      type="button"
                      class="danger"
                      onClick={() => onRevoke(device)}
                      disabled={busy}
                    >
                      Отключить
                    </button>
                  </div>
                </li>
              );
            })}
          </ul>
        )}
      </section>

      {/* How to pair info */}
      <section class="deck-panel guide-deck">
        <h2>Как подключить новое устройство</h2>
        <p>
          Откройте RockMobile или RockCast на устройстве и начните подключение из приложения.
          Браузер подтверждает устройство, но не является RockMobile или RockCast и не считается текущим native-устройством.
        </p>
      </section>

      {/* Security info */}
      <section class="deck-panel guide-deck">
        <h2>Безопасность доступа</h2>
        <p>
          Passkey подтверждает вход в этот браузер. «Отключить» завершает native-сессии выбранного устройства, но не завершает вход в текущем браузере; для него используйте действие ниже.
        </p>
        <p>
          Сервер не удаляет passkey из браузера или Google Password Manager. Старый ключ удаляйте вручную только после успешного входа новым ключом. Одинаковое имя «RockServer user» само по себе не доказывает, что запись старая.
        </p>
      </section>
    </div>
  );
}
