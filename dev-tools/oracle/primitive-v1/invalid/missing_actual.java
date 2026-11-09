// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
class Bad { static void /*D:missing_actual_candidate_one*/m(float a) {} static void /*D:missing_actual_candidate_two*/m(double a) {} static void use() { /*Q:missing_actual*/m(absent); } }
