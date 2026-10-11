# Installing the optional Hot Folder service

Hot Folder has a separate task manager and a background worker. Installing the optional
component registers a system service that starts at boot, before an interactive login.
Its initial configuration has no tasks. Only tasks that you explicitly configure and enable
are processed. Closing the task-manager window does not stop the service.

The worker runs under a restricted account, not root or LocalSystem. That account needs read
access to each input folder and write access to output/archive folders. Moving or deleting
originals also needs write access to the input folder. Prefer dedicated scan folders.
User-session network mounts and encrypted home folders may become available only after login;
configure storage that is available to the service account for unattended processing.

## Windows

Select **Also install PdfCraft Hot Folder** in the MSI. It is unchecked on a new installation.
An upgrade preserves an installed companion unless you deselect it. For unattended installation,
pass `INSTALLHOTFOLDER=1`; pass `INSTALLHOTFOLDER=0` to decline it on a new installation.
The maintenance screen can add or remove the companion without removing PdfCraft.

The **PdfCraft Hot Folder** Start menu entry opens the task file used by the service:
`%ProgramData%\PdfCraft\HotFolder\tasks.json`. Editing the system configuration or changing
service startup requires administrator permission. The service runs as
`NT AUTHORITY\LocalService`; grant that account access to the folders selected for its tasks.
Do not put watched files in the protected program or service-configuration directories.

The MSI registers automatic startup and bounded restart-on-failure recovery. Its task directory
allows administrators and LocalService to write; ordinary users may read status. The separate
`secrets` directory is accessible only to administrators, SYSTEM and LocalService. Credentials
must never be written into exported task JSON. Installer removal stops and unregisters the
service and removes its executable, but retains nonempty task and credential directories.

The portable ZIP includes the companion executable but does not register a service. Use the
MSI when system boot startup is required.

## macOS

Use the `.pkg` installer and select **PdfCraft Hot Folder** in Customize. The selection is off
by default. The standard `.dmg` installs the editor by drag and drop and does not register a
background service.

The optional package installs `/Applications/PdfCraft Hot Folder.app` and a system LaunchDaemon
at `/Library/LaunchDaemons/ai.storyteller.pdfcraft.hot-folder.plist`. It runs under the local
user signed in during installation, even when that user has not yet opened the task manager.
Tasks are stored in that user's `~/Library/Application Support/PdfCraft/HotFolder/tasks.json`.
Use the app as that user to edit tasks. macOS privacy permissions still govern access to
protected folders; a denied access is reported as a task error.

The installer stops an old daemon before replacing the app and preserves existing task data.
To remove boot registration while keeping tasks, run this explicit administrative command:

```sh
sudo '/Applications/PdfCraft Hot Folder.app/Contents/MacOS/PdfCraft Hot Folder' \
  --service-action uninstall \
  --config "$HOME/Library/Application Support/PdfCraft/HotFolder/tasks.json"
```

## Linux

Install the separate `pdfcraft-hot-folder` `.deb` or `.rpm` package alongside the main `pdfcraft`
package. Installing only PdfCraft does not add the service. Hot Folder requires systemd.
The optional package creates the non-login account `pdfcraft-hot-folder`, stores its configuration
in `/var/lib/pdfcraft-hot-folder/tasks.json`, and enables `pdfcraft-hot-folder.service` in the
system service manager. The state directory is owned by that account with mode 0700.

Use administrative access to manage the service configuration; preserve its service-account
ownership and mode 0600. Provision remote-source credentials under the same service account,
or use the task manager with administrative permission. Administrative provisioning requires
an existing private configuration owned by a non-root service account and creates secrets with
that account's ownership; it does not run the document processor as root.
Grant the service account the necessary access to your input/output folders. Removing the optional
package stops and unregisters the service while preserving task data and its account for reinstall.

## Inspecting and controlling the service

The task manager and CLI share the service API. Explicit commands support `status`, `start`,
`stop`, `enable` and `disable`:

```sh
pdfcraft-hot-folder --service-action status --config /absolute/path/to/tasks.json
```

`enable` and `disable` change automatic startup; `start` and `stop` change the current process.
Mutations require the platform's administrative permission. Status refuses to control a service
registered for a different executable or task file. Windows installation/removal belongs to the
MSI. On Linux/macOS, manual registration additionally requires `--account USER`, an existing
non-root account, a protected task file owned by that account, and a root-owned executable.

`--worker --config FILE` is an explicitly launched foreground worker for portable/development
use; it does not install itself. All modes use the same persistent journal and worker lock.
No network listener or MCP server is started by the installer or background worker.

## Validation

Official macOS PKG signing additionally requires a **Developer ID Installer** certificate
(`APPLE_INSTALLER_CERTIFICATE` and `APPLE_INSTALLER_CERTIFICATE_PASSWORD` in release CI),
alongside the application's Developer ID Application certificate. Without signing credentials,
preview apps are ad-hoc signed and their PKG is unsigned and not notarized.

`.github/workflows/hot-folder-services.yml` exercises the real Windows Service Control Manager,
Linux systemd and macOS launchd with a synthetic worker. It verifies boot registration, account,
start/stop, startup enable/disable and graceful cancellation. Installer tests compile the actual
MSI/PKG/DEB definitions around the synthetic worker and verify optional installation, task
preservation and removal. These run only on disposable CI machines. They do not replace testing
the final OCR executable and signed installer artifacts on each supported architecture.
