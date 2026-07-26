; Harness for telefonica-clips/branches/65x/clipsjni/java-src/net/sf/clipsrules/jni/examples/auto/resources/auto_ar.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-9eb5e6b4f011d9d784904861e581bc6f798395b52772fab4b2995ee3593842b9-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-9eb5e6b4f011d9d784904861e581bc6f798395b52772fab4b2995ee3593842b9|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-9eb5e6b4f011d9d784904861e581bc6f798395b52772fab4b2995ee3593842b9|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-9eb5e6b4f011d9d784904861e581bc6f798395b52772fab4b2995ee3593842b9|COMPLETE" crlf))
