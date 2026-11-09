// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
class Bad { void /*D:static_context_instance_candidate_one*/m(float a) {} void /*D:static_context_instance_candidate_two*/m(double a) {} static void use(float f) { /*Q:static_context_instance*/m(f); } }
