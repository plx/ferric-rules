; Harness for telefonica-clips/branches/64x/clipsjni/java-src/net/sf/clipsrules/jni/examples/router/resources/auto_es.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-cb1992ad6e6eb3357be70e8760f7a1a2a068f2deab7f6a9ea82c21f7f206c9fc-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-cb1992ad6e6eb3357be70e8760f7a1a2a068f2deab7f6a9ea82c21f7f206c9fc|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-cb1992ad6e6eb3357be70e8760f7a1a2a068f2deab7f6a9ea82c21f7f206c9fc|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-cb1992ad6e6eb3357be70e8760f7a1a2a068f2deab7f6a9ea82c21f7f206c9fc|COMPLETE" crlf))
