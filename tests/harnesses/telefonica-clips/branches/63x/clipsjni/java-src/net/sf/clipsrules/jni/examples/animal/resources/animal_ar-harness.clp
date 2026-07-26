; Harness for telefonica-clips/branches/63x/clipsjni/java-src/net/sf/clipsrules/jni/examples/animal/resources/animal_ar.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-9c152aa34ab0058b9d4d1d80f0fe3d3eda1f5a692d65da571a5af5e0f2849da5-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-9c152aa34ab0058b9d4d1d80f0fe3d3eda1f5a692d65da571a5af5e0f2849da5|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-9c152aa34ab0058b9d4d1d80f0fe3d3eda1f5a692d65da571a5af5e0f2849da5|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-9c152aa34ab0058b9d4d1d80f0fe3d3eda1f5a692d65da571a5af5e0f2849da5|COMPLETE" crlf))
