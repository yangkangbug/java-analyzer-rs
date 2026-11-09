// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
class Bad { void /*D:class_qualified_instance_candidate_one*/m(float a) {} void /*D:class_qualified_instance_candidate_two*/m(double a) {} static void use(float f) { Bad./*Q:class_qualified_instance*/m(f); } }
