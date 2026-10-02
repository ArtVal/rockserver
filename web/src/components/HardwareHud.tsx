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

export const formatDate = (value: string) => {
  try {
    return new Intl.DateTimeFormat("ru-RU", {
      day: "numeric",
      month: "short",
      hour: "2-digit",
      minute: "2-digit",
    }).format(new Date(value));
  } catch {
    return value;
  }
};

/**
 * Hardware HUD component rendering connected RockCast and RockMobile client devices
 * as cabinet cards with the product type, the session-derived status, connection
 * dates, rename/revoke controls, and QR pairing guidance.
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
    <div class="hardware-hud">
      <section class="deck-panel hardware-deck" aria-label="Подключённые устройства">
        <div class="panel-header-row">
          <div class="hardware-title-wrap">
            <span class="hud-status-dot glow-cyan" />
            <h2>Подключённые устройства</h2>
          </div>
          <span
            class="device-limit-pill font-mono-code"
            title={`Подключено ${account.devices.length} из ${account.device_limit} устройств`}
          >
            {account.devices.length} / {account.device_limit}
          </span>
        </div>

        {limitReached && (
          <p class="limit-alert" role="status">
            ⚠ Лимит устройств достигнут ({account.device_limit}). Сначала отключите старое устройство.
          </p>
        )}

        {account.devices.length === 0 ? (
          <div class="devices-empty" role="status">
            <strong class="devices-empty-title">Подключённых устройств пока нет</strong>
            <p class="devices-empty-text">
              Начните подключение в приложении RockCast или RockMobile: оно покажет QR-код.
              Отсканируйте его камерой телефона и подтвердите запрос в открывшемся браузере —
              устройство появится в этом списке.
            </p>
          </div>
        ) : (
          <ul class="devices" aria-label="Список подключённых устройств">
            {account.devices.map((device) => {
              const fresh =
                justConnected?.device_display_name === device.device_display_name &&
                justConnected.device_type === device.device_type;
              const busy = deviceBusy === device.device_id;
              const isActive = device.session_status === "active";
              const isMobile = device.device_type.toLowerCase().includes("mobile");

              return (
                <li key={device.device_id} class="device-card-item">
                  {fresh && (
                    <p class="fresh" role="status">
                      ✓ Только что подключено
                    </p>
                  )}
                  <div class="device-card-header">
                    <div class="device-identity">
                      <span class="device-type-icon" aria-hidden="true">
                        {isMobile ? "📱" : "💻"}
                      </span>
                      <div class="device-name-wrap">
                        <strong class="device-name-text" title={deviceName(device)}>
                          {deviceName(device)}
                        </strong>
                        <span class="device-product-tag font-mono-code">
                          {deviceProductName(device.device_type)}
                        </span>
                      </div>
                    </div>
                    <span class={`device-status-badge ${isActive ? "active" : "inactive"}`}>
                      <span class={`status-circle ${isActive ? "active" : "inactive"}`} />
                      {isActive ? "В сети" : "Офлайн"}
                    </span>
                  </div>

                  <div class="device-meta-list">
                    <div class="device-meta-entry">
                      <span class="meta-label">Подключено</span>
                      <span class="meta-val">{formatDate(device.connected_at)}</span>
                    </div>
                    {device.last_seen_at ? (
                      <div class="device-meta-entry">
                        <span class="meta-label">Активность</span>
                        <span class="meta-val">{formatDate(device.last_seen_at)}</span>
                      </div>
                    ) : (
                      <div class="device-meta-entry">
                        <span class="meta-label">Активность</span>
                        <span class="meta-val meta-unknown">нет данных</span>
                      </div>
                    )}
                  </div>

                  <div class="device-actions">
                    <button
                      type="button"
                      class="device-action-btn secondary"
                      onClick={() => onRename(device)}
                      disabled={busy}
                    >
                      {busy ? "Обновляем…" : "Переименовать"}
                    </button>
                    <button
                      type="button"
                      class="device-action-btn danger"
                      onClick={() => onRevoke(device)}
                      disabled={busy}
                    >
                      {busy ? "Обновляем…" : "Отключить"}
                    </button>
                  </div>
                </li>
              );
            })}
          </ul>
        )}
      </section>

      {/* How to pair info */}
      <section class="deck-panel guide-deck" aria-label="Как подключить новое устройство">
        <h2>Как подключить новое устройство</h2>
        <ol class="guide-steps">
          <li>
            Откройте RockCast или RockMobile на устройстве и начните подключение — приложение
            покажет QR-код, проверочную фразу и короткий код.
          </li>
          <li>Отсканируйте QR-код камерой телефона: откроется защищённая ссылка этого запроса.</li>
          <li>
            Сверьте фразу и код с экраном устройства и подтвердите подключение в браузере.
            Браузер подтверждает устройство, но не является RockMobile или RockCast и не считается
            текущим native-устройством.
          </li>
        </ol>
      </section>

      {/* Security info */}
      <section class="deck-panel guide-deck" aria-label="Безопасность доступа">
        <h2>Безопасность доступа</h2>
        <p>
          Passkey подтверждает вход в этот браузер. «Отключить» завершает native-сессии выбранного
          устройства, но не завершает вход в текущем браузере; для него используйте действие ниже.
        </p>
        <p>
          Сервер не удаляет passkey из браузера или Google Password Manager. Старый ключ удаляйте
          вручную только после успешного входа новым ключом. Одинаковое имя «RockServer user» само
          по себе не доказывает, что запись старая.
        </p>
      </section>
    </div>
  );
}
