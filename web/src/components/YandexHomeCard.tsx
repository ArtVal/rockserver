import { useEffect, useState } from "preact/hooks";
import { api, type ApiError, type BrowserAccount, type YandexHomeDevice } from "../api";
import { formatDate } from "./HardwareHud";

/**
 * Yandex Smart Home sensor and telemetry card.
 * Handles OAuth connection, listing connected sensors (climate, temperature,
 * humidity, CO2, voltage, battery), and manual disconnect.
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
  const [loading, setLoading] = useState(account.yandex_home_connected);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState(() => {
    if (initialStatus === "failed") return "Не удалось подключить Яндекс Дом. Повторите попытку.";
    if (initialStatus === "connected") return "Яндекс Дом успешно подключён.";
    return "";
  });

  const load = async () => {
    setLoading(true);
    setMessage("");
    try {
      const res = await api.yandexHomeSensors();
      setDevices(res.devices ?? []);
    } catch (error) {
      setDevices([]);
      setMessage(
        (error as ApiError)?.code === "yandex_home_reconnect_required"
          ? "Доступ к Яндекс Дому истёк. Подключите его снова."
          : "Не удалось получить данные Яндекс Дома."
      );
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    if (account.yandex_home_connected) void load();
    else {
      setDevices([]);
      setLoading(false);
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

  return (
    <section class="deck-panel yandex-home-deck">
      <div class="panel-header-row">
        <div>
          <p class="eyebrow">Умный дом</p>
          <h2>Яндекс Алиса</h2>
        </div>
        {account.yandex_home_connected && (
          <button
            type="button"
            class="link-button refresh-telemetry-btn font-mono-code text-xs"
            onClick={() => void load()}
            disabled={loading || busy}
          >
            {loading ? "ОБНОВЛЕНИЕ…" : "ОБНОВИТЬ"}
          </button>
        )}
      </div>

      {account.yandex_home_connected ? (
        <>
          <p class="yandex-connected-hint">
            Яндекс Дом подключён. Показания датчиков и устройств:
          </p>
          {loading ? (
            <p role="status">Обновляем показания…</p>
          ) : devices.length ? (
            <ul class="sensors" aria-label="Датчики и устройства Яндекс Дома">
              {devices.map((device) => {
                const latestUpdate = device.properties
                  .map((p) => p.updated_at)
                  .filter((t): t is string => Boolean(t))
                  .sort()
                  .reverse()[0];

                return (
                  <li key={device.id} class="sensor-card">
                    <div class="sensor-header">
                      <strong>{device.name}</strong>
                      {device.room_name && (
                        <small class="sensor-room">{device.room_name}</small>
                      )}
                    </div>
                    <div class="sensor-properties">
                      {device.properties.map((prop) => (
                        <div key={prop.instance} class="sensor-property-row">
                          <span class="sensor-prop-name">{prop.name}:</span>
                          <span class="sensor-prop-val">{prop.formatted_value}</span>
                        </div>
                      ))}
                    </div>
                    {latestUpdate && (
                      <small class="sensor-timestamp">
                        Обновлено {formatDate(latestUpdate)}
                      </small>
                    )}
                  </li>
                );
              })}
            </ul>
          ) : (
            !message && <p role="status">Датчиков с доступными показаниями не найдено.</p>
          )}

          <div class="yandex-actions-row">
            <button
              type="button"
              class="secondary"
              onClick={() => void load()}
              disabled={loading || busy}
            >
              {loading ? "Обновляем…" : "Обновить показания"}
            </button>
            <button
              type="button"
              class="danger"
              onClick={() => void disconnect()}
              disabled={busy}
            >
              {busy ? "Отключаем…" : "Отключить Яндекс Дом"}
            </button>
          </div>
        </>
      ) : (
        <>
          <p>
            Подключите аккаунт Яндекса, чтобы видеть показания датчиков, доступных в приложении «Дом с Алисой».
          </p>
          <button
            type="button"
            onClick={() => void connect()}
            disabled={busy || !csrf}
          >
            {busy ? "Открываем Яндекс…" : "Подключить Яндекс Дом"}
          </button>
        </>
      )}

      {message && <p role="alert">{message}</p>}
    </section>
  );
}
