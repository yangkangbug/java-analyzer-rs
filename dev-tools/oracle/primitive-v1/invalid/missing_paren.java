// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
class Bad { static void /*D:missing_paren_candidate_one*/m(float a) {} static void /*D:missing_paren_candidate_two*/m(double a) {} static void use(float f) { /*Q:missing_paren*/m(f; } }
