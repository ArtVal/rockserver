import { type PairingPreview } from "../api";
import { deviceName, deviceProductName, formatDate } from "./HardwareHud";

export type PairingState =
  | "loading"
  | "anonymous"
  | "authenticated"
  | "approving"
  | "approved"
  | "cancelled"
  | "terminal"
  | "unavailable";

export interface RegistrationViewProps {
  registrationComplete: boolean;
  authenticatedAccountName: string;
  registrationName: string;
  authBusy: boolean;
  message: string;
  setRegistrationName: (name: string) => void;
  onUseDefaultName: () => void;
  onRegister: () => void;
  onReturn: () => void;
}

/** Renders the passkey registration screen reachable from pairing and the landing page. */
export function RegistrationView({
  registrationComplete,
  authenticatedAccountName,
  registrationName,
  authBusy,
  message,
  setRegistrationName,
  onUseDefaultName,
  onRegister,
  onReturn,
}: RegistrationViewProps) {
  return (
    <main className="pairing-screen">
      <header>
        <span>ROCK</span>
        <h1>Создать Rock-аккаунт</h1>
      </header>
      {registrationComplete ? (
        <section>
          <p className="eyebrow">Аккаунт создан</p>
          <h2>Вход в браузере выполнен</h2>
          <p>Rock-аккаунт «{authenticatedAccountName}» создан и защищён passkey.</p>
          <a className="button" href="/">
            Открыть аккаунт и устройства
          </a>
        </section>
      ) : (
        <section>
          <p>
            Создайте новый Rock-аккаунт с passkey. Для входа в существующий аккаунт используйте отдельный вход.
          </p>
          <label htmlFor="account-name">
            Имя аккаунта <span className="example">Например, Алексей</span>
          </label>
          <input
            id="account-name"
            value={registrationName}
            maxLength={128}
            placeholder="Например, Алексей"
            disabled={authBusy}
            onInput={event => setRegistrationName(event.currentTarget.value)}
          />
          <button
            className="secondary"
            onClick={onUseDefaultName}
            disabled={authBusy}
          >
            Использовать «Rock account»
          </button>
          <button onClick={onRegister} disabled={authBusy}>
            {authBusy ? "Создаём…" : "Создать аккаунт с passkey"}
          </button>
          <button
            className="link-button"
            onClick={onReturn}
            disabled={authBusy}
          >
            У меня уже есть аккаунт
          </button>
        </section>
      )}
      {message && <p role="alert">{message}</p>}
      <footer>Passkey и данные сессии не сохраняются в браузере.</footer>
    </main>
  );
}

export interface PairingViewProps {
  preview?: PairingPreview;
  pairingState: PairingState;
  authenticatedAccountName: string;
  authBusy: boolean;
  message: string;
  onApprove: () => void;
  onCancel: () => void;
  onRetry: () => void;
  onAuthenticate: () => void;
  onOpenRegistration: () => void;
  onOpenCabinet: () => void;
}

/** Renders the standalone mobile pairing screen opened from a scanned QR code. */
export function PairingView({
  preview,
  pairingState,
  authenticatedAccountName,
  authBusy,
  message,
  onApprove,
  onCancel,
  onRetry,
  onAuthenticate,
  onOpenRegistration,
  onOpenCabinet,
}: PairingViewProps) {
  const deviceType = deviceProductName(preview?.device_type ?? "");
  return (
    <main className="pairing-screen">
      <header>
        <span>ROCK</span>
        <h1>Подключение устройства</h1>
      </header>
      {preview && pairingState !== "approved" && (
        <section aria-label="Данные запроса подключения">
          <p className="eyebrow">Проверьте, что это ваше устройство</p>
          <h2>{deviceName(preview)}</h2>
          <dl className="pairing-facts">
            <div className="pairing-fact">
              <dt>Проверочная фраза</dt>
              <dd
                className="pairing-phrase"
                aria-label={`Проверочная фраза: ${preview.verification_phrase}`}
              >
                {preview.verification_phrase}
              </dd>
            </div>
            <div className="pairing-fact">
              <dt>Короткий код</dt>
              <dd
                className="pairing-code"
                aria-label={`Короткий код: ${preview.short_code}`}
              >
                {preview.short_code}
              </dd>
            </div>
            <div className="pairing-fact">
              <dt>Действует до</dt>
              <dd>{formatDate(preview.expires_at)}</dd>
            </div>
          </dl>
        </section>
      )}
      {message && <p role="alert">{message}</p>}
      {pairingState === "approved" && preview ? (
        <section aria-live="polite">
          <p className="eyebrow pairing-success">✓ Устройство подключено</p>
          <h2>
            {deviceName(preview)} подключён к «{authenticatedAccountName}»
          </h2>
          {deviceType === "RockMobile" ? (
            <a className="button" href="/return/rockmobile">
              Вернуться в RockMobile
            </a>
          ) : (
            <p role="status">Вернитесь в RockCast или закройте браузер.</p>
          )}
          <button className="secondary" onClick={onOpenCabinet}>
            Открыть аккаунт и устройства
          </button>
        </section>
      ) : pairingState === "cancelled" ? (
        <section aria-live="polite">
          <h2>Подключение отменено</h2>
          <p>Запрос не подтверждён. Вернитесь в {deviceType}, если захотите начать заново.</p>
          <button className="secondary" onClick={onOpenCabinet}>
            Открыть аккаунт и устройства
          </button>
        </section>
      ) : pairingState === "terminal" ? (
        <section>
          <h2>Ссылка подключения недействительна</h2>
          <p>
            Запрос больше нельзя подтвердить из этого браузера. Откройте новую защищённую
            ссылку на устройстве, которое хотите подключить.
          </p>
        </section>
      ) : pairingState === "unavailable" ? (
        <section>
          <h2>Сервис недоступен</h2>
          <p>Не удалось связаться с сервером. Проверьте подключение и повторите попытку.</p>
          <button onClick={onRetry}>Повторить попытку</button>
        </section>
      ) : !preview ? (
        <section aria-live="polite">
          <h2>
            <span className="pairing-spinner" aria-hidden="true" />
            Загружаем данные подключения…
          </h2>
          <p>Проверяем ссылку. Дождитесь загрузки, прежде чем считать её недействительной.</p>
        </section>
      ) : pairingState === "loading" ? (
        <section aria-live="polite">
          <h2>
            <span className="pairing-spinner" aria-hidden="true" />
            Проверяем вашу сессию…
          </h2>
          <p>После проверки сессии показанное устройство можно будет подключить.</p>
        </section>
      ) : pairingState === "authenticated" || pairingState === "approving" ? (
        <section>
          <h2>Подключить {deviceType} к аккаунту «{authenticatedAccountName}»?</h2>
          <p>Будет подключено только показанное выше устройство.</p>
          <button onClick={onApprove} disabled={pairingState !== "authenticated"}>
            {pairingState === "approving" ? "Подключаем…" : "Подключить"}
          </button>
          <button className="secondary" onClick={onCancel}>
            Отмена
          </button>
        </section>
      ) : (
        <section>
          <h2>
            {authenticatedAccountName
              ? `Подтвердите вход в «${authenticatedAccountName}»`
              : "Чтобы продолжить"}
          </h2>
          <p>
            {authenticatedAccountName
              ? "Сессия завершена. Войдите с passkey, чтобы подключить устройство."
              : "Войдите в существующий Rock-аккаунт или создайте новый. После этого вы вернётесь к этому устройству."}
          </p>
          <button onClick={onAuthenticate} disabled={authBusy}>
            {authBusy
              ? "Проверяем…"
              : authenticatedAccountName
                ? "Подтвердить passkey"
                : "Войти с passkey"}
          </button>
          {authenticatedAccountName ? (
            <>
              <p>
                Если passkey этого аккаунта удалён, восстановить его без сохранённого ключа нельзя.
              </p>
              <button className="secondary" onClick={onOpenRegistration} disabled={authBusy}>
                Создать другой Rock-аккаунт
              </button>
            </>
          ) : (
            <button className="secondary" onClick={onOpenRegistration} disabled={authBusy}>
              Создать Rock-аккаунт
            </button>
          )}
        </section>
      )}
      {preview && pairingState !== "approved" && (
        <p className="pairing-note">
          Сверьте фразу и код с экраном {deviceType}. Подключайте только своё устройство.
        </p>
      )}
      <footer>Passkey и данные сессии не сохраняются в браузере.</footer>
    </main>
  );
}
