; Harness for telefonica-clips/branches/65x/clipsjni/java-src/net/sf/clipsrules/jni/examples/router/resources/animal_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-4d873d83c18561478dd0216b7a6709bc5393869592031067e3ec5844f2142f23-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-4d873d83c18561478dd0216b7a6709bc5393869592031067e3ec5844f2142f23|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-4d873d83c18561478dd0216b7a6709bc5393869592031067e3ec5844f2142f23|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-4d873d83c18561478dd0216b7a6709bc5393869592031067e3ec5844f2142f23|COMPLETE" crlf))
