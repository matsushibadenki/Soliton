"""HTTP handoff/mutual-exclusion checks, without GPU or HTTP server imports."""
import importlib.util
import io
import urllib.error
from pathlib import Path
import threading
import time
import unittest

spec = importlib.util.spec_from_file_location('prefix_http', Path(__file__).with_name('prefix-cache-http.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

controller_spec=importlib.util.spec_from_file_location('prefix_qualification',Path(__file__).with_name('qualify-prefix-cache.py'))
controller=importlib.util.module_from_spec(controller_spec)
controller_spec.loader.exec_module(controller)


class Admission(unittest.TestCase):
    def test_http_error_diagnostics_preserve_authentication_status(self):
        error=urllib.error.HTTPError('http://127.0.0.1',401,'Unauthorized',{},io.BytesIO(b'{"error":"Authentication required"}'))
        controller.annotate_http_error(error)
        self.assertEqual(error.code,401)
        self.assertTrue(error.reason.startswith('Unauthorized'))
        self.assertIn('Authentication required',error.reason)

    def test_serial_client_can_wait_for_previous_handler_teardown(self):
        admission = module.RequestAdmission()
        self.assertTrue(admission.acquire())
        timer = threading.Timer(.02, admission.release)
        timer.start()
        try:
            self.assertTrue(admission.acquire())
            admission.release()
        finally: timer.join()

    def test_long_active_request_is_refused_without_releasing_its_slot(self):
        admission = module.RequestAdmission()
        self.assertTrue(admission.acquire())
        start = time.monotonic()
        self.assertFalse(admission.acquire())
        self.assertLess(time.monotonic()-start, .5)
        self.assertTrue(admission.lock.locked())
        admission.release()
        self.assertTrue(admission.acquire())
        admission.release()

    def test_multiple_waiters_never_execute_in_parallel(self):
        admission = module.RequestAdmission()
        active = 0
        peak = 0
        errors = []
        def work():
            nonlocal active, peak
            if not admission.acquire(): errors.append('unexpected refusal'); return
            try:
                active += 1
                peak = max(peak, active)
                time.sleep(.005)
                active -= 1
            finally: admission.release()
        threads = [threading.Thread(target=work) for _ in range(8)]
        for t in threads: t.start()
        for t in threads: t.join()
        self.assertEqual(errors, [])
        self.assertEqual(peak, 1)


if __name__ == '__main__': unittest.main()
