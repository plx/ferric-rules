; Harness for telefonica-clips/branches/64x/clipsjni/java-src/net/sf/clipsrules/jni/examples/router/resources/auto_ar.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-aab8cade6cc5adb1269d7f5d876f7294c3d07aa4d13c07edf2d777b7318963e7-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-aab8cade6cc5adb1269d7f5d876f7294c3d07aa4d13c07edf2d777b7318963e7|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-aab8cade6cc5adb1269d7f5d876f7294c3d07aa4d13c07edf2d777b7318963e7|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-aab8cade6cc5adb1269d7f5d876f7294c3d07aa4d13c07edf2d777b7318963e7|COMPLETE" crlf))
