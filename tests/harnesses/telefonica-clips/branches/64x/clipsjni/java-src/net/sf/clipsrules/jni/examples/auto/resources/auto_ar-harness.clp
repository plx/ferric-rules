; Harness for telefonica-clips/branches/64x/clipsjni/java-src/net/sf/clipsrules/jni/examples/auto/resources/auto_ar.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-620977749b877ff0073afd670715a6da70fdb3a45c682253b912040c4b0be729-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-620977749b877ff0073afd670715a6da70fdb3a45c682253b912040c4b0be729|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-620977749b877ff0073afd670715a6da70fdb3a45c682253b912040c4b0be729|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-620977749b877ff0073afd670715a6da70fdb3a45c682253b912040c4b0be729|COMPLETE" crlf))
