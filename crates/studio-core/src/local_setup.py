# Executed inside Strata's isolated venv. Inputs are a validated, owned symlink view.
import sys
import importlib.util
from pathlib import Path


def configure(metal, stage, data):
    """Keep legacy setup and the shared 0.2.x Mac planner in local-only mode."""
    S = metal.S
    original_done, original_download = S.done, S.download
    # A missing .done sidecar in another app's store must never cause a re-download.
    S.done = lambda p: True if Path(p).parent == stage and Path(p).is_file() else original_done(p)

    def download(url, destination, *args, **kwargs):
        if not str(destination).lower().endswith(('.zip', '.tar.gz', '.tar.xz')):
            raise RuntimeError('Local reuse forbids model/auxiliary weight downloads: ' + str(destination))
        return original_download(url, destination, *args, **kwargs)

    S.download = download

    # 0.2.x data_folder_for passes migration/default-location flags to data_folder.
    # Neither signature may move another app's files or update upstream settings.
    def data_folder(*_args, **_kwargs):
        data.mkdir(parents=True, exist_ok=True)
        return data, []

    S.data_folder = data_folder
    if hasattr(S, 'data_folder_for'):
        S.data_folder_for = data_folder
    # The 0.2.x engine_args delegates to mac_fit, whose own globals choose MTP.
    # Disable it at both call sites, including large/resident Macs, without fetching weights.
    metal.stream_mtp = lambda *_args: False
    if hasattr(metal, '_fit'):
        metal._fit.stream_mtp = metal.stream_mtp


def main():
    root, stage, data = map(Path, sys.argv[1:4])
    sys.path.insert(0, str(root))
    spec = importlib.util.spec_from_file_location('studio_metal_setup', root / 'metal/setup_mac.py')
    metal = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(metal)
    configure(metal, stage, data)
    return metal.main(['--yes', '--setup', '--no-start', '--no-browser', '--vision', 'no',
                       '--model', sys.argv[4], '--context', sys.argv[5],
                       '--gguf-dir', str(stage), '--data-dir', str(data)])


if __name__ == '__main__':
    raise SystemExit(main())
