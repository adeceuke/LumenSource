import { useEffect, useState } from "react";
import { desktopCommands, messageFromError } from "../commands";
import { browserMessages } from "../i18n";
import { emptyRemoteTarget } from "../modelUi";
import type {
  RemoteTargetConfig,
  RemoteTargetProfile,
} from "../types";
import { RemoteTargetDialog } from "./AppDialogs";
import { MachineDetails, type MachineDefinition } from "./MachineDetails";

const text = browserMessages();

function localizedError(error: unknown): string {
  return messageFromError(error, text.errors.unexpected);
}

export function MachinesPage() {
  const [targets, setTargets] = useState<RemoteTargetProfile[]>([]);
  const [selectedTargetId, setSelectedTargetId] = useState<string>();
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string>();
  const [dialogOpen, setDialogOpen] = useState(false);
  const [dialogSaving, setDialogSaving] = useState(false);
  const [dialogError, setDialogError] = useState<string>();
  const [config, setConfig] = useState<RemoteTargetConfig>(emptyRemoteTarget);
  const [password, setPassword] = useState("");
  const [rememberPassword, setRememberPassword] = useState(true);
  const [sessionPasswords, setSessionPasswords] = useState<Record<string, string>>({});

  useEffect(() => {
    void desktopCommands.loadRemoteTargets()
      .then((profiles) => {
        setTargets(profiles);
        setError(undefined);
      })
      .catch((failure: unknown) => setError(localizedError(failure)))
      .finally(() => setLoading(false));
  }, []);

  const machines: MachineDefinition[] = [
    {
      targetId: "local",
      name: text.machines.localName,
    },
    ...targets.map((target) => ({
      targetId: target.targetId,
      name: target.targetName,
      config: target.config,
    })),
  ];
  const selectedMachine = machines.find((machine) => machine.targetId === selectedTargetId);

  const openDialog = () => {
    setConfig(emptyRemoteTarget());
    setPassword("");
    setRememberPassword(true);
    setDialogError(undefined);
    setDialogOpen(true);
  };

  const closeDialog = () => {
    if (dialogSaving) return;
    setDialogOpen(false);
    setDialogError(undefined);
    setPassword("");
  };

  const saveMachine = async () => {
    setDialogSaving(true);
    setDialogError(undefined);
    try {
      const saved = await desktopCommands.saveRemoteTarget({
        ...config,
        name: config.name.trim(),
        host: config.host.trim(),
        username: config.username.trim(),
        identityFile: config.authentication === "key"
          ? config.identityFile?.trim() || undefined
          : undefined,
      });
      setTargets((current) => [
        ...current.filter((target) => target.targetId !== saved.targetId),
        saved,
      ].sort((left, right) => left.targetName.localeCompare(right.targetName)));
      if (saved.config.authentication === "password" && password) {
        if (rememberPassword) {
          await desktopCommands.saveRemotePassword(saved.targetId, password);
        }
        setSessionPasswords((current) => ({ ...current, [saved.targetId]: password }));
      }
      setSelectedTargetId(saved.targetId);
      setDialogOpen(false);
      setConfig(emptyRemoteTarget());
      setPassword("");
    } catch (failure) {
      setDialogError(localizedError(failure));
    } finally {
      setDialogSaving(false);
    }
  };

  if (selectedMachine) {
    return (
      <MachineDetails
        key={selectedMachine.targetId}
        machine={selectedMachine}
        password={sessionPasswords[selectedMachine.targetId]}
        onPassword={async (value, save) => {
          if (save) {
            await desktopCommands.saveRemotePassword(selectedMachine.targetId, value);
          }
          setSessionPasswords((current) => ({
            ...current,
            [selectedMachine.targetId]: value,
          }));
        }}
        onForgetPassword={async () => {
          await desktopCommands.deleteRemotePassword(selectedMachine.targetId);
          setSessionPasswords((current) => {
            const next = { ...current };
            delete next[selectedMachine.targetId];
            return next;
          });
        }}
        onBack={() => setSelectedTargetId(undefined)}
        errorFrom={localizedError}
      />
    );
  }

  return (
    <>
      <div className="models-toolbar">
        <div>
          <span className="eyebrow">{text.machines.eyebrow}</span>
          <h1>{text.machines.title}</h1>
        </div>
        <button className="primary-button" type="button" onClick={openDialog}>
          {text.machines.add}
        </button>
      </div>

      {error && <div className="inline-error" role="alert">{error}</div>}

      <div className="models-table" role="table" aria-label={text.machines.tableLabel}>
        {machines.map((machine) => (
          <div className="models-table-row machine-table-row" role="row" key={machine.targetId}>
            <button
              className="model-row-button"
              type="button"
              onClick={() => setSelectedTargetId(machine.targetId)}
              aria-label={text.machines.openDetails(machine.name)}
            >
              <div className="model-main">
                <span className={`machine-kind-icon ${machine.targetId === "local" ? "local" : "remote"}`}>
                  {machine.targetId === "local" ? "L" : "R"}
                </span>
                <div>
                  <strong role="cell">{machine.name}</strong>
                  <p>
                    {machine.config
                      ? text.machines.remoteDescription(
                        machine.config.username,
                        machine.config.host,
                        machine.config.port,
                      )
                      : text.machines.localDescription}
                  </p>
                </div>
              </div>
            </button>
            <span className="machine-row-arrow" aria-hidden="true">›</span>
          </div>
        ))}
        {loading && (
          <div className="details-empty compact">
            {text.machines.loading}
          </div>
        )}
      </div>

      {dialogOpen && (
        <RemoteTargetDialog
          context="machines"
          config={config}
          password={password}
          rememberPassword={rememberPassword}
          saving={dialogSaving}
          error={dialogError}
          setConfig={setConfig}
          setPassword={setPassword}
          setRememberPassword={setRememberPassword}
          onClose={closeDialog}
          onSave={() => void saveMachine()}
        />
      )}
    </>
  );
}
