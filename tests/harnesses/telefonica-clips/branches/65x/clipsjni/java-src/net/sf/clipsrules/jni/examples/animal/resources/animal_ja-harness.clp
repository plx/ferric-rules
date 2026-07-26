; Harness for telefonica-clips/branches/65x/clipsjni/java-src/net/sf/clipsrules/jni/examples/animal/resources/animal_ja.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-f0ae45dbbd4c14b574c797a43d438490686cb609fc1c3626d4fd74bdf615c296-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-f0ae45dbbd4c14b574c797a43d438490686cb609fc1c3626d4fd74bdf615c296|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-f0ae45dbbd4c14b574c797a43d438490686cb609fc1c3626d4fd74bdf615c296|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-f0ae45dbbd4c14b574c797a43d438490686cb609fc1c3626d4fd74bdf615c296|COMPLETE" crlf))
