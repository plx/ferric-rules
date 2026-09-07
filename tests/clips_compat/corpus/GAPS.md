# Compatibility discovery issue index

These issues record the original observations of checkout `eb24cc50`.
Some issues have since been fixed or consolidated; this index preserves their
discovery history. Consult [manifest.json](manifest.json) for the current active
gaps and exact Ferric observations; `.out` always records CLIPS 6.30 behavior.
A future fix must remove/update its manifest characterization.

On engine snapshot `d428e780`, seven formerly divergent cases now conform to
CLIPS. Expression fact queries are now explicitly rejected, so their two
previously matching empty-result controls also carry characterizations under
[#324](https://github.com/plx/ferric-rules/issues/324). The current totals are
158 conformance cases and 61 active gap cases. Historical issue descriptions
below are not a substitute for the updated manifest's exact observations.

27 distinct new issues were opened during this discovery pass; existing tracked
gaps were linked without opening duplicates. No engine repairs are included.

| Issue | Behavior at discovery | Cases |
|---|---|---|
| [#320](https://github.com/plx/ferric-rules/issues/320) | ordered fact patterns do not enforce field cardinality | [facts/003_ordered_exact_arity.clp](facts/003_ordered_exact_arity.clp), [facts/016_ordered_empty_pattern_arity.clp](facts/016_ordered_empty_pattern_arity.clp), [patterns/007_anonymous_single_field.clp](patterns/007_anonymous_single_field.clp) |
| [#321](https://github.com/plx/ferric-rules/issues/321) | ordered multifield variables before fixed fields fail to match | [patterns/010_multifield_middle_capture.clp](patterns/010_multifield_middle_capture.clp), [patterns/030_multifield_prefix_capture.clp](patterns/030_multifield_prefix_capture.clp), [patterns/031_multifield_middle_empty.clp](patterns/031_multifield_middle_empty.clp), [patterns/035_multifield_split.clp](patterns/035_multifield_split.clp) |
| [#322](https://github.com/plx/ferric-rules/issues/322) | template multislot patterns bind scalar values instead of matching sequences | [patterns/011_template_multislot_capture.clp](patterns/011_template_multislot_capture.clp), [patterns/032_template_multislot_whole_capture.clp](patterns/032_template_multislot_whole_capture.clp), [patterns/033_template_multislot_single_field.clp](patterns/033_template_multislot_single_field.clp) |
| [#323](https://github.com/plx/ferric-rules/issues/323) | bare unrestricted defmethod parameters are rejected | [generics/bare-method-parameter.clp](generics/bare-method-parameter.clp), [generics/unrestricted-fallback.clp](generics/unrestricted-fallback.clp) |
| [#324](https://github.com/plx/ferric-rules/issues/324) | fact-query expressions return empty defaults despite matching facts | [queries/any-factp-match.clp](queries/any-factp-match.clp), [queries/any-factp-filter.clp](queries/any-factp-filter.clp), [queries/find-fact-first.clp](queries/find-fact-first.clp), [queries/find-all-facts-count.clp](queries/find-all-facts-count.clp), [queries/find-all-facts-filter.clp](queries/find-all-facts-filter.clp) |
| [#325](https://github.com/plx/ferric-rules/issues/325) | action query predicates cannot evaluate query slot references | [queries/do-for-all-facts-filter.clp](queries/do-for-all-facts-filter.clp), [queries/query-cartesian-product.clp](queries/query-cartesian-product.clp) |
| [#326](https://github.com/plx/ferric-rules/issues/326) | fact-query traversal does not preserve assertion order | [queries/do-for-fact-first.clp](queries/do-for-fact-first.clp), [queries/do-for-all-facts-order.clp](queries/do-for-all-facts-order.clp) |
| [#327](https://github.com/plx/ferric-rules/issues/327) | query-bound fact variables cannot be retracted inside query bodies | [queries/delayed-query-retract.clp](queries/delayed-query-retract.clp) |
| [#328](https://github.com/plx/ferric-rules/issues/328) | fact-address pattern bindings are unavailable to introspection expressions | [queries/fact-index-address.clp](queries/fact-index-address.clp), [queries/fact-existence-lifecycle.clp](queries/fact-existence-lifecycle.clp), [queries/fact-relation-ordered.clp](queries/fact-relation-ordered.clp), [queries/fact-relation-template.clp](queries/fact-relation-template.clp), [queries/fact-slot-value-single.clp](queries/fact-slot-value-single.clp), [queries/fact-slot-value-multislot.clp](queries/fact-slot-value-multislot.clp), [queries/fact-slot-value-implied.clp](queries/fact-slot-value-implied.clp), [queries/fact-slot-names-template.clp](queries/fact-slot-names-template.clp), [queries/fact-slot-names-implied.clp](queries/fact-slot-names-implied.clp) |
| [#329](https://github.com/plx/ferric-rules/issues/329) | fact-index exposes internal slot-map identity instead of assertion index | [queries/query-fact-index.clp](queries/query-fact-index.clp) |
| [#330](https://github.com/plx/ferric-rules/issues/330) | allow local variable binding and parameter rebinding in deffunctions | [procedural/007_function_local_bind.clp](procedural/007_function_local_bind.clp), [procedural/008_function_parameter_rebind.clp](procedural/008_function_parameter_rebind.clp) |
| [#331](https://github.com/plx/ferric-rules/issues/331) | accept case clauses inside switch actions | [procedural/033_switch_matching_case.clp](procedural/033_switch_matching_case.clp), [procedural/034_switch_default.clp](procedural/034_switch_default.clp), [procedural/035_switch_type_sensitive.clp](procedural/035_switch_type_sensitive.clp) |
| [#332](https://github.com/plx/ferric-rules/issues/332) | preserve selected operand types in min and max | [stdlib/009_minimum_mixed_types.clp](stdlib/009_minimum_mixed_types.clp), [stdlib/010_maximum_mixed_types.clp](stdlib/010_maximum_mixed_types.clp) |
| [#333](https://github.com/plx/ferric-rules/issues/333) | match round at positive half-integer ties | [stdlib/011_round_half_boundaries.clp](stdlib/011_round_half_boundaries.clp) |
| [#334](https://github.com/plx/ferric-rules/issues/334) | support variadic numeric comparisons | [stdlib/046_numeric_equal_chain.clp](stdlib/046_numeric_equal_chain.clp), [stdlib/047_numeric_less_chain.clp](stdlib/047_numeric_less_chain.clp), [stdlib/048_numeric_greater_chain.clp](stdlib/048_numeric_greater_chain.clp), [stdlib/049_numeric_inequality_chain.clp](stdlib/049_numeric_inequality_chain.clp), [stdlib/065_numeric_less_equal_chain.clp](stdlib/065_numeric_less_equal_chain.clp), [stdlib/066_numeric_greater_equal_chain.clp](stdlib/066_numeric_greater_equal_chain.clp) |
| [#335](https://github.com/plx/ferric-rules/issues/335) | accept SYMBOL arguments to str-length | [stdlib/045_string_length_symbol.clp](stdlib/045_string_length_symbol.clp) |
| [#336](https://github.com/plx/ferric-rules/issues/336) | clamp substring start indices below one | [stdlib/036_substring_clipped_bounds.clp](stdlib/036_substring_clipped_bounds.clp) |
| [#337](https://github.com/plx/ferric-rules/issues/337) | match str-index for an empty search string | [stdlib/038_string_index_empty_needle.clp](stdlib/038_string_index_empty_needle.clp) |
| [#338](https://github.com/plx/ferric-rules/issues/338) | parse only the first token in string-to-field | [stdlib/041_string_to_field_first_token.clp](stdlib/041_string_to_field_first_token.clp) |
| [#339](https://github.com/plx/ferric-rules/issues/339) | preserve quoted fields when parsing explode$ | [stdlib/042_explode_quoted_fields.clp](stdlib/042_explode_quoted_fields.clp) |
| [#340](https://github.com/plx/ferric-rules/issues/340) | honor zero padding in format integer directives | [stdlib/067_format_zero_padding.clp](stdlib/067_format_zero_padding.clp) |
| [#341](https://github.com/plx/ferric-rules/issues/341) | return nil for out-of-range nth$ indices | [stdlib/053_nth_out_of_bounds.clp](stdlib/053_nth_out_of_bounds.clp), [stdlib/069_nth_negative_index.clp](stdlib/069_nth_negative_index.clp), [stdlib/070_nth_excessive_index.clp](stdlib/070_nth_excessive_index.clp) |
| [#342](https://github.com/plx/ferric-rules/issues/342) | search multifield subsequences with member$ | [stdlib/055_member_subsequence.clp](stdlib/055_member_subsequence.clp) |
| [#343](https://github.com/plx/ferric-rules/issues/343) | use CLIPS ordering for builtin sort predicates | [stdlib/061_sort_comparator_direction.clp](stdlib/061_sort_comparator_direction.clp) |
| [#344](https://github.com/plx/ferric-rules/issues/344) | preserve quoted STRING fields in implode$ | [stdlib/063_implode_preserves_string_quotes.clp](stdlib/063_implode_preserves_string_quotes.clp) |
| [#345](https://github.com/plx/ferric-rules/issues/345) | quote STRING elements when printing multifields | [stdlib/068_printout_multifield_string_quotes.clp](stdlib/068_printout_multifield_string_quotes.clp) |
| [#346](https://github.com/plx/ferric-rules/issues/346) | read truncates quoted strings at whitespace | [io/read-string.clp](io/read-string.clp) |

Existing issues: [#103](https://github.com/plx/ferric-rules/issues/103) (duplicate facts), [#104](https://github.com/plx/ferric-rules/issues/104) (leading not), [#192](https://github.com/plx/ferric-rules/issues/192) (immediate focus, consolidated into #157), and [#299](https://github.com/plx/ferric-rules/issues/299) (template defaults/identity).
