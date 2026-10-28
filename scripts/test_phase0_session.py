"""Synthetic checks for renderer evidence: enumeration is not selection."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("phase0", Path(__file__).with_name("phase0-session.py"))
phase0 = importlib.util.module_from_spec(spec)
spec.loader.exec_module(phase0)


class AdapterEvidence(unittest.TestCase):
    def test_uses_selected_adapter_not_available_hardware(self):
        log = '''Available adapters: [AdapterInfo { name: "Discrete GPU" }]
INFO iced_wgpu: Selected: AdapterInfo {
    name: "llvmpipe",
    device_type: Cpu,
    backend: Vulkan,
}
'''
        self.assertEqual(phase0.selected_adapter(log), {
            "name": "llvmpipe", "device_type": "Cpu", "backend": "Vulkan"
        })

    def test_missing_or_partial_evidence_fails(self):
        for log in ('Available adapters: [GPU]', 'Selected: AdapterInfo {\nname: "GPU",\n}'):
            with self.subTest(log=log), self.assertRaises(RuntimeError):
                phase0.selected_adapter(log)


if __name__ == "__main__":
    unittest.main()
