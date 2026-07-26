; Harness for telefonica-clips/branches/65x/clipsjni/java-src/net/sf/clipsrules/jni/examples/router/resources/animal_ar.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-7fc481437a98c7fde06937e3f0ece45ae85cd5e67ed7de9c96741780f3f027b1-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-7fc481437a98c7fde06937e3f0ece45ae85cd5e67ed7de9c96741780f3f027b1|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-7fc481437a98c7fde06937e3f0ece45ae85cd5e67ed7de9c96741780f3f027b1|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-7fc481437a98c7fde06937e3f0ece45ae85cd5e67ed7de9c96741780f3f027b1|COMPLETE" crlf))
