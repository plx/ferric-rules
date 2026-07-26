; Harness for telefonica-clips/branches/64x/clipsjni/java-src/net/sf/clipsrules/jni/examples/animal/resources/animal_ar.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-b2906e020400c3c16fc98ab76c52efcacf3e01c3e06c07643a60d1bf7813a29f-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-b2906e020400c3c16fc98ab76c52efcacf3e01c3e06c07643a60d1bf7813a29f|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-b2906e020400c3c16fc98ab76c52efcacf3e01c3e06c07643a60d1bf7813a29f|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-b2906e020400c3c16fc98ab76c52efcacf3e01c3e06c07643a60d1bf7813a29f|COMPLETE" crlf))
