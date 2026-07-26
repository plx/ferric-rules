; Harness for telefonica-clips/branches/64x/clipsjni/java-src/net/sf/clipsrules/jni/examples/animal/resources/animal.clp
; Detected constructs: deffacts: MAIN::knowledge-base
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-55c9920f303b385187d06f1701c8a5ac7fcb7de8bf76eb86ec9218fc0aa75795-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-55c9920f303b385187d06f1701c8a5ac7fcb7de8bf76eb86ec9218fc0aa75795|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-55c9920f303b385187d06f1701c8a5ac7fcb7de8bf76eb86ec9218fc0aa75795|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-55c9920f303b385187d06f1701c8a5ac7fcb7de8bf76eb86ec9218fc0aa75795|COMPLETE" crlf))
