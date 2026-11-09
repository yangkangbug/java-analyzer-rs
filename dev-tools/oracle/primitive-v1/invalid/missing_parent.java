// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
class Bad extends Missing { static void /*D:missing_parent_candidate_one*/m(float a) {} static void /*D:missing_parent_candidate_two*/m(double a) {} static void use(float f) { /*Q:missing_parent*/m(f); } }
