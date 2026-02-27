; Harness for telefonica-clips/branches/65x/test_suite/templerr.clp
; Detected constructs: deftemplate: bad-foo01, bad-foo02, bad-foo03, bad-foo04, aok-foo01, aok-foo02, aok-foo03, aok-foo04, bad-foo05, bad-foo06, bad-foo07, bad-foo08, aok-foo05, bad-foo09, bad-foo10, bad-foo11, bad-foo12, bad-foo13, bad-foo13, bad-foo12, aok-foo06, aok-foo07, aok-foo08, aok-foo09, bad-foo13, bad-foo14, bad-foo15, bad-foo16, bad-foo17, bad-foo18, bad-foo19, bad-foo20, bad-foo21, bad-foo22, bad-foo23, bad-foo24, aok-foo10, aok-foo11, aok-foo12, aok-foo13, aok-foo14, aok-foo15, aok-foo16, aok-foo17, aok-foo18, aok-foo19, aok-foo20, aok-foo21, bad-foo25, bad-foo26, bad-foo27, bad-foo28, bad-foo29, bad-foo30, bad-foo31, bad-foo32, aok-foo21a, aok-foo21b, aok-foo21c, aok-foo21d, bad-foo37, bad-foo38, bad-foo39, bad-foo40, bad-foo41, bad-foo42, bad-foo43, bad-foo44, bad-foo45, bad-foo46, bad-foo47, bad-foo48, bad-foo49, bad-foo50, bad-foo51, bad-foo52, bad-foo53, bad-foo54, bad-foo55, bad-foo56, bad-foo57, bad-foo58, bad-foo59, bad-foo60, bad-foo61, bad-foo62, bad-foo63, bad-foo64, bad-foo65, bad-foo66, bad-foo67, bad-foo68, bad-foo69, bad-foo70, bad-foo71, bad-foo72, bad-foo73, bad-foo74, bad-foo75, bad-foo76, bad-foo77, bad-foo78, bad-foo79, bad-foo80, bad-foo81, bad-foo82, bad-foo83, bad-foo84, bad-foo85, bad-foo86, bad-foo87, bad-foo88, bad-foo89, bad-foo90, bad-foo91, bad-foo92, aok-foo22, aok-foo23, aok-foo24, aok-foo25, aok-foo26, aok-foo27, aok-foo28, aok-foo29, aok-foo30, aok-foo31, aok-foo32, aok-foo33, aok-foo34, aok-foo35, aok-foo36, aok-foo37, aok-foo38, aok-foo39, aok-foo40, aok-foo41, aok-foo42, aok-foo43, aok-foo44, aok-foo45, aok-foo46, bad-foo93, bad-foo94, aok-foo47
;
; Strategy: verify file loads and reset succeeds.
; The source file is loaded via (load ...) before this harness.

(defrule harness-verify
   (initial-fact)
   =>
   (printout t "HARNESS: loaded" crlf))
