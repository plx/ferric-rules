; Harness for telefonica-clips/branches/65x/clipsjni/java-src/net/sf/clipsrules/jni/examples/animal/resources/animal.clp
; Detected constructs: deffacts: MAIN::knowledge-base
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-ac52e120137bb47f9e8ff172cea91b2db3be30410d6347e251a45bfdb0500d4a-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-ac52e120137bb47f9e8ff172cea91b2db3be30410d6347e251a45bfdb0500d4a|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-ac52e120137bb47f9e8ff172cea91b2db3be30410d6347e251a45bfdb0500d4a|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-ac52e120137bb47f9e8ff172cea91b2db3be30410d6347e251a45bfdb0500d4a|COMPLETE" crlf))
