"""Deterministic CPU mathematical checks; no real checkpoints/network required."""
import unittest
import torch
from mamba_reference import Mamba, affine_scan, selective_scan_reference


def sequential(u, delta, A, B, C, D, z, bias):
    dt = torch.nn.functional.softplus(delta + bias[None, :, None])
    h = torch.zeros(u.shape[0], u.shape[1], A.shape[1])
    ys = []
    for index in range(u.shape[-1]):
        h = torch.exp(dt[:, :, index, None] * A) * h + dt[:, :, index, None] * B[:, None, :, index] * u[:, :, index, None]
        ys.append((h * C[:, None, :, index]).sum(-1))
    y = torch.stack(ys, -1) + D[None, :, None] * u
    return y * torch.nn.functional.silu(z), h


class MambaReferenceTests(unittest.TestCase):
    def test_associative_prefix_matches_scalar_recurrence(self):
        a = torch.tensor([.5, .25, .8, .9, .2]).reshape(1, 1, 5, 1)
        b = torch.tensor([1., -2., 3., .5, -.75]).reshape(1, 1, 5, 1)
        product, values = affine_scan(a, b)
        state, decay, expected, products = 0., 1., [], []
        for av, bv in zip(a.flatten(), b.flatten()):
            state = av * state + bv; decay = av * decay
            expected.append(state); products.append(decay)
        torch.testing.assert_close(values.flatten(), torch.stack(expected))
        torch.testing.assert_close(product.flatten(), torch.stack(products))

    def test_scan_matches_independent_loop_across_chunk_boundaries(self):
        torch.manual_seed(9)
        u, delta, z = [torch.randn(2, 5, 139) for _ in range(3)]
        A = -torch.rand(5, 3)
        B, C = [torch.randn(2, 3, 139) for _ in range(2)]
        D, bias = torch.randn(5), torch.randn(5)
        expected, state = sequential(u, delta, A, B, C, D, z, bias)
        for chunk_size in (1, 16, 64, 256):
            actual, last = selective_scan_reference(u, delta, A, B, C, D, z, bias, True, True, chunk_size)
            torch.testing.assert_close(actual, expected, rtol=2e-5, atol=1e-5)
            torch.testing.assert_close(last, state, rtol=2e-5, atol=1e-5)

    def test_module_roundtrip_has_expected_standard_mamba_shapes(self):
        model = Mamba(64, d_state=16, d_conv=4, expand=4).eval()
        expected = {'in_proj.weight': (512,64), 'conv1d.weight': (256,1,4), 'conv1d.bias': (256,),
                    'x_proj.weight': (36,256), 'dt_proj.weight': (256,4), 'dt_proj.bias': (256,),
                    'A_log': (256,16), 'D': (256,), 'out_proj.weight': (64,256)}
        self.assertEqual({key:tuple(value.shape) for key,value in model.state_dict().items()}, expected)
        replica = Mamba(64, d_state=16, d_conv=4, expand=4).eval()
        replica.load_state_dict(model.state_dict(), strict=True)
        x = torch.randn(1, 9, 64)
        with torch.inference_mode():
            torch.testing.assert_close(model(x), replica(x))
            self.assertTrue(torch.isfinite(model(x)).all())

    def test_invalid_layout_and_streaming_cache_rejected(self):
        model = Mamba(4)
        with self.assertRaises(ValueError): model(torch.zeros(1, 2, 4), inference_params={})
        with self.assertRaises(ValueError): affine_scan(torch.zeros(2), torch.zeros(2))
        with self.assertRaises(ValueError): selective_scan_reference(torch.zeros(1,2,3),torch.zeros(1,2,3),torch.zeros(2,4),torch.zeros(2,4),torch.zeros(2,4))


if __name__ == '__main__': unittest.main()
