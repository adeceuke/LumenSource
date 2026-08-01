import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
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
  const [editingTargetId, setEditingTargetId] = useState<string>();
  const [menuOpenId, setMenuOpenId] = useState<string>();
  const [menuPosition, setMenuPosition] = useState<{ top: number; left: number }>();
  const [config, setConfig] = useState<RemoteTargetConfig>(emptyRemoteTarget);
  const [password, setPassword] = useState("");
  const [rememberPassword, setRememberPassword] = useState(true);
  const [editingPasswordSaved, setEditingPasswordSaved] = useState(false);
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

  useEffect(() => {
    if (!menuOpenId) return;
    const dismissMenu = (event: PointerEvent) => {
      const target = event.target;
      if (target instanceof Element && target.closest("[data-machine-menu]")) return;
      setMenuOpenId(undefined);
      setMenuPosition(undefined);
    };
    const dismissWithKeyboard = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      setMenuOpenId(undefined);
      setMenuPosition(undefined);
    };
    document.addEventListener("pointerdown", dismissMenu, true);
    document.addEventListener("keydown", dismissWithKeyboard);
    return () => {
      document.removeEventListener("pointerdown", dismissMenu, true);
      document.removeEventListener("keydown", dismissWithKeyboard);
    };
  }, [menuOpenId]);

  useEffect(() => {
    let disposed = false;
    setEditingPasswordSaved(false);
    if (!dialogOpen || !editingTargetId) return () => {
      disposed = true;
    };
    void desktopCommands.remoteCredentialStatus(editingTargetId)
      .then((status) => {
        if (!disposed) setEditingPasswordSaved(status.passwordSaved);
      })
      .catch(() => {
        if (!disposed) setEditingPasswordSaved(false);
      });
    return () => {
      disposed = true;
    };
  }, [dialogOpen, editingTargetId]);

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
    setEditingTargetId(undefined);
    setConfig(emptyRemoteTarget());
    setPassword("");
    setRememberPassword(true);
    setEditingPasswordSaved(false);
    setDialogError(undefined);
    setDialogOpen(true);
  };

  const openEditDialog = (target: RemoteTargetProfile) => {
    setEditingTargetId(target.targetId);
    setConfig({ ...target.config });
    setPassword(sessionPasswords[target.targetId] ?? "");
    setRememberPassword(true);
    setDialogError(undefined);
    setMenuOpenId(undefined);
    setMenuPosition(undefined);
    setDialogOpen(true);
  };

  const closeDialog = () => {
    if (dialogSaving) return;
    setDialogOpen(false);
    setDialogError(undefined);
    setPassword("");
    setEditingTargetId(undefined);
  };

  const saveMachine = async () => {
    setDialogSaving(true);
    setDialogError(undefined);
    try {
      const normalizedConfig = {
        ...config,
        name: config.name.trim(),
        host: config.host.trim(),
        username: config.username.trim(),
        identityFile: config.authentication === "key"
          ? config.identityFile?.trim() || undefined
          : undefined,
      };
      const saved = editingTargetId
        ? await desktopCommands.updateRemoteTarget(editingTargetId, normalizedConfig)
        : await desktopCommands.saveRemoteTarget(normalizedConfig);
      setTargets((current) => [
        ...current.filter((target) => target.targetId !== saved.targetId),
        saved,
      ].sort((left, right) => left.targetName.localeCompare(right.targetName)));
      if (saved.config.authentication === "password" && password) {
        if (rememberPassword) {
          await desktopCommands.saveRemotePassword(saved.targetId, password);
        } else {
          await desktopCommands.deleteRemotePassword(saved.targetId);
        }
      } else if (saved.config.authentication === "password" && !rememberPassword) {
        await desktopCommands.deleteRemotePassword(saved.targetId);
      }
      setSessionPasswords((current) => {
        const next = { ...current };
        if (editingTargetId && editingTargetId !== saved.targetId) delete next[editingTargetId];
        if (password) next[saved.targetId] = password;
        return next;
      });
      setSelectedTargetId(saved.targetId);
      setDialogOpen(false);
      setConfig(emptyRemoteTarget());
      setPassword("");
      setEditingTargetId(undefined);
    } catch (failure) {
      setDialogError(localizedError(failure));
    } finally {
      setDialogSaving(false);
    }
  };

  const toggleMenu = (targetId: string, trigger: HTMLButtonElement) => {
    if (menuOpenId === targetId) {
      setMenuOpenId(undefined);
      setMenuPosition(undefined);
      return;
    }
    const rect = trigger.getBoundingClientRect();
    const menuHeight = 92;
    setMenuOpenId(targetId);
    setMenuPosition({
      top: rect.bottom + 8 + menuHeight <= window.innerHeight
        ? rect.bottom + 8
        : Math.max(8, rect.top - menuHeight - 8),
      left: Math.max(8, Math.min(rect.right - 160, window.innerWidth - 168)),
    });
  };

  const removeMachine = async (target: RemoteTargetProfile) => {
    setMenuOpenId(undefined);
    setMenuPosition(undefined);
    if (!window.confirm(text.machines.removeConfirmation(target.targetName))) return;
    try {
      await desktopCommands.removeRemoteTarget(target.targetId);
      setTargets((current) => current.filter((item) => item.targetId !== target.targetId));
      setSessionPasswords((current) => {
        const next = { ...current };
        delete next[target.targetId];
        return next;
      });
      setError(undefined);
    } catch (failure) {
      setError(localizedError(failure));
    }
  };

  const editedTarget = editingTargetId
    ? targets.find((target) => target.targetId === editingTargetId)
    : undefined;
  const passwordRequired = config.authentication === "password"
    && (!editedTarget || editedTarget.config.authentication !== "password");

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
            {machine.targetId === "local" ? (
              <span className="machine-row-arrow" aria-hidden="true">›</span>
            ) : (
              <div className="model-actions">
                <div className="menu-wrap" data-machine-menu>
                  <button
                    className="icon-button"
                    type="button"
                    aria-label={text.machines.menuAria(machine.name)}
                    aria-haspopup="menu"
                    aria-expanded={menuOpenId === machine.targetId}
                    onClick={(event) => toggleMenu(machine.targetId, event.currentTarget)}
                  >
                    ⋯
                  </button>
                  {menuOpenId === machine.targetId && menuPosition && createPortal(
                    <div className="menu-popover" data-machine-menu role="menu" style={{ top: `${menuPosition.top}px`, left: `${menuPosition.left}px` }}>
                      <button type="button" role="menuitem" onClick={() => openEditDialog(targets.find((target) => target.targetId === machine.targetId)!)}>{text.machines.edit}</button>
                      <button type="button" role="menuitem" onClick={() => void removeMachine(targets.find((target) => target.targetId === machine.targetId)!)}>{text.machines.remove}</button>
                    </div>,
                    document.body,
                  )}
                </div>
              </div>
            )}
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
          editing={Boolean(editingTargetId)}
          config={config}
          password={password}
          passwordSaved={editingPasswordSaved}
          rememberPassword={rememberPassword}
          passwordRequired={passwordRequired}
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
