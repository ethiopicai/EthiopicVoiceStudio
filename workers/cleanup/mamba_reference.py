# SPDX-License-Identifier: Apache-2.0
# State-space parameter layout and recurrence adapted from state-spaces/mamba
# v2.2.4, Copyright (c) 2023, Tri Dao, Albert Gu.
# Adaptation: pure-Torch bounded associative scan; no fused kernel imports.
# Upstream license: licenses/mamba-Apache-2.0.txt
"""Experimental pure-PyTorch Mamba-1 inference for the RE-USE checkpoint.

State-dict layout follows upstream state-spaces/mamba mamba_simple.Mamba.
No fused/CUDA imports are required. The scan implements the same real-valued
recurrence with associative block prefixes, so floating-point operation ordering
differs from the CUDA kernel. Full-model numerical/quality parity is a separate
validation requirement; this file does not establish a supported Mac runtime.
"""
from __future__ import annotations

import math
import torch
from torch import nn
from torch.nn import functional as F


def affine_scan(decay: torch.Tensor, drive: torch.Tensor) -> tuple[torch.Tensor, torch.Tensor]:
    """Inclusive affine prefix along axis 2: h[t] = decay[t]*h[t-1]+drive[t].

    Returns cumulative decay and zero-initial-state drive. Inputs [B,D,L,N]
    must have equal shapes. Prefix doubling avoids a Python loop per frame.
    """
    if decay.shape != drive.shape or decay.ndim != 4:
        raise ValueError("Affine scan expects equal [batch, feature, time, state] tensors")
    a, b = decay, drive
    offset = 1
    while offset < decay.shape[2]:
        # Compose later affine transform with the preceding prefix. Concatenation
        # preserves the old arrays throughout the current doubling step.
        b = torch.cat((b[:, :, :offset], b[:, :, offset:] + a[:, :, offset:] * b[:, :, :-offset]), dim=2)
        a = torch.cat((a[:, :, :offset], a[:, :, offset:] * a[:, :, :-offset]), dim=2)
        offset *= 2
    return a, b


def selective_scan_reference(u, delta, A, B, C, D=None, z=None,
                             delta_bias=None, delta_softplus=False,
                             return_last_state=False, chunk_size=64):
    """Real variable-B/C Mamba selective scan, retaining bounded frame blocks.

    u/delta: [batch,inner,time], A: [inner,state], B/C: [batch,state,time].
    This is the exact parameter layout used by the pinned bidirectional RE-USE
    model; complex/grouped SSM variants and recurrent inference caches are not
    supported. State accumulation is float32 as in upstream selective_scan_ref.
    """
    if chunk_size < 1:
        raise ValueError("chunk_size must be positive")
    if u.ndim != 3 or delta.shape != u.shape or A.ndim != 2:
        raise ValueError("Invalid selective scan input shapes")
    batch, inner, length = u.shape
    if A.shape[0] != inner or B.shape != (batch, A.shape[1], length) or C.shape != B.shape:
        raise ValueError("Expected input-dependent B/C with [batch,state,time] layout")
    if A.is_complex() or B.is_complex() or C.is_complex():
        raise ValueError("Complex SSMs are not supported")
    original_dtype = u.dtype
    u, delta, A, B, C = (value.float() for value in (u, delta, A, B, C))
    if delta_bias is not None:
        delta = delta + delta_bias.float()[None, :, None]
    if delta_softplus:
        delta = F.softplus(delta)
    state = A.new_zeros((batch, inner, A.shape[1]))
    pieces = []
    # Bound temporary expanded scan arrays. Long/high-rate recordings should
    # still be chunked by the worker because encoder activations are separate.
    # Each expanded float32 [B,D,L,N] array stays under roughly 16 MiB
    # unless one frame itself exceeds that budget. Prefix composition holds
    # several such arrays transiently, independently of total recording length.
    chunk_size = min(chunk_size, max(1, 4_194_304 // max(1, batch * inner * A.shape[1])))
    for start in range(0, length, chunk_size):
        end = min(start + chunk_size, length)
        dt = delta[:, :, start:end, None]
        decay = torch.exp(dt * A[None, :, None, :])
        drive = dt * B[:, None, :, start:end].transpose(2, 3) * u[:, :, start:end, None]
        cumulative, values = affine_scan(decay, drive)
        values = values + cumulative * state[:, :, None, :]
        state = values[:, :, -1, :]
        pieces.append((values * C[:, None, :, start:end].transpose(2, 3)).sum(dim=-1))
    output = torch.cat(pieces, dim=2) if pieces else u.new_empty((batch, inner, 0))
    if D is not None:
        output = output + u * D.float()[None, :, None]
    if z is not None:
        output = output * F.silu(z.float())
    output = output.to(original_dtype)
    return (output, state) if return_last_state else output


class Mamba(nn.Module):
    """Inference-only Mamba-1 module compatible with RE-USE state dictionaries."""
    def __init__(self, d_model, d_state=16, d_conv=4, expand=2, dt_rank="auto",
                 dt_min=0.001, dt_max=0.1, dt_init="random", dt_scale=1.0,
                 dt_init_floor=1e-4, conv_bias=True, bias=False,
                 use_fast_path=False, layer_idx=None, device=None, dtype=None):
        super().__init__()
        if min(d_model, d_state, d_conv) < 1 or expand <= 0:
            raise ValueError("Mamba dimensions must be positive")
        self.d_model, self.d_state, self.d_conv, self.expand = d_model, d_state, d_conv, expand
        self.d_inner = int(expand * d_model)
        self.dt_rank = math.ceil(d_model / 16) if dt_rank == "auto" else dt_rank
        self.layer_idx = layer_idx
        factory = {"device": device, "dtype": dtype}
        self.in_proj = nn.Linear(d_model, self.d_inner * 2, bias=bias, **factory)
        self.conv1d = nn.Conv1d(self.d_inner, self.d_inner, d_conv, groups=self.d_inner,
                                padding=d_conv - 1, bias=conv_bias, **factory)
        self.x_proj = nn.Linear(self.d_inner, self.dt_rank + 2 * d_state, bias=False, **factory)
        self.dt_proj = nn.Linear(self.dt_rank, self.d_inner, bias=True, **factory)
        std = self.dt_rank ** -0.5 * dt_scale
        if dt_init == "constant":
            nn.init.constant_(self.dt_proj.weight, std)
        elif dt_init == "random":
            nn.init.uniform_(self.dt_proj.weight, -std, std)
        else:
            raise ValueError("Unknown delta initialization")
        dt = torch.exp(torch.rand(self.d_inner, **factory) * (math.log(dt_max) - math.log(dt_min)) + math.log(dt_min)).clamp(min=dt_init_floor)
        with torch.no_grad():
            self.dt_proj.bias.copy_(dt + torch.log(-torch.expm1(-dt)))
        self.A_log = nn.Parameter(torch.arange(1, d_state + 1, device=device, dtype=torch.float32).log().repeat(self.d_inner, 1))
        self.D = nn.Parameter(torch.ones(self.d_inner, device=device, dtype=torch.float32))
        self.out_proj = nn.Linear(self.d_inner, d_model, bias=bias, **factory)

    def forward(self, hidden_states, inference_params=None):
        if inference_params is not None:
            raise ValueError("Streaming Mamba cache is not supported by this bidirectional reference")
        if hidden_states.ndim != 3 or hidden_states.shape[-1] != self.d_model:
            raise ValueError("Expected [batch,time,d_model] input")
        if hidden_states.shape[1] == 0:
            return hidden_states.new_empty(hidden_states.shape)
        x, z = self.in_proj(hidden_states).transpose(1, 2).chunk(2, dim=1)
        x = F.silu(self.conv1d(x)[..., :hidden_states.shape[1]])
        delta, B, C = self.x_proj(x.transpose(1, 2)).split([self.dt_rank, self.d_state, self.d_state], dim=-1)
        # Upstream applies dt_proj.bias inside the scan before softplus, once.
        delta = F.linear(delta, self.dt_proj.weight).transpose(1, 2)
        y = selective_scan_reference(x, delta, -self.A_log.float().exp(), B.transpose(1, 2), C.transpose(1, 2),
                                     self.D, z, self.dt_proj.bias, True)
        return self.out_proj(y.transpose(1, 2))


# Familiar upstream name for callers replacing only the scan implementation.
selective_scan_ref = selective_scan_reference
