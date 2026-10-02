import { useEffect, useState } from "preact/hooks";
import { api, type ApiError, type BrowserAccount, type YandexHomeDevice } from "../api";
import { formatDate } from "./HardwareHud";

type YandexView = "loading" | "ready" | "empty" | "error" | "expired";

const STATUS_LABEL: Record<YandexView | "disconnected", string> = {
  disconnected: "Не подключён",
  loading: "Загрузка",
  ready: "Подключён",
  empty: "Нет датчиков",
  error: "Ошибка данных",
  expired: "Доступ истёк",
};

/** Renders the property value as-is and appends the unit only when it is not already inside it. */
const propertyValue = (prop: YandexHomeDevice["properties"][number]) => {
  const unit = prop.unit?.trim() ?? "";
  if (!unit || prop.formatted_value.includes(unit)) return prop.formatted_value;
  return `${prop.formatted_value} ${unit}`;
};

/**
 * Yandex Smart Home card in the devices section. Keeps the existing OAuth connect
 * flow, sensor telemetry with rooms, units and update times, manual refresh, and a
 * confirmed disconnect, while rendering one distinct card per connection state.
 */
export function YandexHomeCard({
  account,
  csrf,
  onChanged,
  initialStatus,
}: {
  account: BrowserAccount;
  csrf: string;
  onChanged: () => Promise<void>;
  initialStatus?: string;
}) {
  const [devices, setDevices] = useState<YandexHomeDevice[]>([]);
  const [view, setView] = useState<YandexView>("loading");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState(() => {
    if (initialStatus === "failed") return "Не удалось подключить Яндекс Дом. Повторите попытку.";
    if (initialStatus === "connected") return "Яндекс Дом успешно подключён.";
    return "";
  });

  // `userInitiated` keeps the OAuth return message visible after the automatic mount load;
  // explicit refresh/retry actions clear it because they start a new interaction.
  const load = async (userInitiated = true) => {
    setView("loading");
    if (userInitiated) setMessage("");
    try {
      const res = await api.yandexHomeSensors();
      setDevices(res.devices ?? []);
      setView((res.devices ?? []).length ? "ready" : "empty");
    } catch (error) {
      setDevices([]);
      if ((error as ApiError)?.code === "yandex_home_reconnect_required") {
        // The server has already revoked the dead connection; only a fresh OAuth pass helps.
        setView("expired");
      } else {
        setView("error");
      }
    }
  };

  useEffect(() => {
    if (account.yandex_home_connected) void load(false);
    else {
      setDevices([]);
      setView("loading");
    }
  }, [account.yandex_home_connected]);

  const connect = async () => {
    setBusy(true);
    setMessage("");
    try {
      location.assign((await api.yandexHomeAuthorization(csrf)).authorization_url);
    } catch {
      setMessage("Не удалось открыть подключение Яндекс Дома. Повторите позже.");
      setBusy(false);
    }
  };

  const disconnect = async () => {
    if (
      !window.confirm(
        "Отключить Яндекс Дом? RockServer перестанет видеть показания датчиков."
      )
    )
      return;
    setBusy(true);
    setMessage("");
    try {
      await api.disconnectYandexHome(csrf);
      await onChanged();
    } catch {
      setMessage("Не удалось отключить Яндекс Дом. Повторите позже.");
    } finally {
      setBusy(false);
    }
  };

  const statusLabel = account.yandex_home_connected
    ? STATUS_LABEL[view]
    : STATUS_LABEL.disconnected;

  return (
    <section class="deck-panel yandex-home-deck" aria-label="Яндекс Дом">
      <div class="panel-header-row">
        <div class="yh-heading">
          <p class="eyebrow">Дом с Алисой</p>
          <h2>Яндекс Дом</h2>
        </div>
        <span class={`yh-state-badge yh-state-${account.yandex_home_connected ? view : "disconnected"}`}>
          {statusLabel}
        </span>
      </div>

      <div class="yh-body" aria-live="polite">
        {!account.yandex_home_connected ? (
          <p class="yh-hint">
            Подключите аккаунт Яндекса, чтобы видеть показания датчиков из приложения
            «Дом с Алисой».
          </p>
        ) : view === "loading" ? (
          <p class="yh-hint" role="status">
            Обновляем показания…
          </p>
        ) : view === "expired" ? (
          <p class="yh-hint">Доступ к Яндекс Дому истёк. Подключите его снова.</p>
        ) : view === "error" ? (
          <p class="yh-hint">Не удалось получить данные Яндекс Дома.</p>
        ) : view === "empty" ? (
          <p class="yh-hint">Датчиков с доступными показаниями не найдено.</p>
        ) : (
          <ul class="sensors" aria-label="Датчики и устройства Яндекс Дома">
            {devices.map((device) => (
              <li key={device.id} class="sensor-card">
                <div class="sensor-header">
                  <strong>{device.name}</strong>
                  {device.room_name && (
                    <small class="sensor-room">{device.room_name}</small>
                  )}
                </div>
                {device.properties.length ? (
                  <div class="sensor-properties">
                    {device.properties.map((prop) => (
                      <div key={prop.instance} class="sensor-property-row">
                        <span class="sensor-prop-name">{prop.name}</span>
                        <span class="sensor-prop-val">
                          {propertyValue(prop)}
                          {prop.updated_at && (
                            <small class="sensor-prop-time" title={`Обновлено ${formatDate(prop.updated_at)}`}>
                              {formatDate(prop.updated_at)}
                            </small>
                          )}
                        </span>
                      </div>
                    ))}
                  </div>
                ) : (
                  <p class="sensor-no-props">Доступных показаний нет</p>
                )}
              </li>
            ))}
          </ul>
        )}
      </div>

      <div class="yandex-actions-row">
        {!account.yandex_home_connected || view === "expired" ? (
          <button
            type="button"
            onClick={() => void connect()}
            disabled={busy || !csrf}
          >
            {busy
              ? "Открываем Яндекс…"
              : view === "expired"
              ? "Подключить снова"
              : "Подключить Яндекс Дом"}
          </button>
        ) : view === "loading" ? null : view === "error" ? (
          <button type="button" onClick={() => void load()} disabled={busy}>
            Повторить
          </button>
        ) : (
          <>
            <button
              type="button"
              class="secondary"
              onClick={() => void load()}
              disabled={busy}
            >
              Обновить показания
            </button>
            <button type="button" class="danger" onClick={() => void disconnect()} disabled={busy}>
              {busy ? "Отключаем…" : "Отключить Яндекс Дом"}
            </button>
          </>
        )}
      </div>

      {message && <p role="alert">{message}</p>}
    </section>
  );
}
