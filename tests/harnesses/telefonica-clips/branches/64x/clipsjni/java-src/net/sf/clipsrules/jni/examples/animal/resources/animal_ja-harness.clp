; Harness for telefonica-clips/branches/64x/clipsjni/java-src/net/sf/clipsrules/jni/examples/animal/resources/animal_ja.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-bb060e2ac000f1b5fc3422d59626f0a52c6cc29c82f193b4b7a2fddd2cc3f484-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-bb060e2ac000f1b5fc3422d59626f0a52c6cc29c82f193b4b7a2fddd2cc3f484|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-bb060e2ac000f1b5fc3422d59626f0a52c6cc29c82f193b4b7a2fddd2cc3f484|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-bb060e2ac000f1b5fc3422d59626f0a52c6cc29c82f193b4b7a2fddd2cc3f484|COMPLETE" crlf))
