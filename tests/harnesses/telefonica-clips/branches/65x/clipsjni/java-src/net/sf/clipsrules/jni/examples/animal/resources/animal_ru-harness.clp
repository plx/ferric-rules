; Harness for telefonica-clips/branches/65x/clipsjni/java-src/net/sf/clipsrules/jni/examples/animal/resources/animal_ru.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-1f5f562429e75812622e4f4b2e69248e924872d8e317f931591501343296d438-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-1f5f562429e75812622e4f4b2e69248e924872d8e317f931591501343296d438|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-1f5f562429e75812622e4f4b2e69248e924872d8e317f931591501343296d438|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-1f5f562429e75812622e4f4b2e69248e924872d8e317f931591501343296d438|COMPLETE" crlf))
