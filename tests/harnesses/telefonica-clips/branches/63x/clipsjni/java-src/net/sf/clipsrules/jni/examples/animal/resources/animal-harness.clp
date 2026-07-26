; Harness for telefonica-clips/branches/63x/clipsjni/java-src/net/sf/clipsrules/jni/examples/animal/resources/animal.clp
; Detected constructs: deffacts: MAIN::knowledge-base
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-52dca52c06a75a224cb0561b97f2b14c79bcd9f75b727cf01bee9df4a692b54c-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-52dca52c06a75a224cb0561b97f2b14c79bcd9f75b727cf01bee9df4a692b54c|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-52dca52c06a75a224cb0561b97f2b14c79bcd9f75b727cf01bee9df4a692b54c|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-52dca52c06a75a224cb0561b97f2b14c79bcd9f75b727cf01bee9df4a692b54c|COMPLETE" crlf))
