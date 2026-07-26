; Harness for telefonica-clips/branches/64x/clipsjni/java-src/net/sf/clipsrules/jni/examples/auto/resources/auto_ja.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-c73b53e1dbb03e252dd9dc1ad8a559c2b09867676f76de13f93b0cd6718110b3-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-c73b53e1dbb03e252dd9dc1ad8a559c2b09867676f76de13f93b0cd6718110b3|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-c73b53e1dbb03e252dd9dc1ad8a559c2b09867676f76de13f93b0cd6718110b3|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-c73b53e1dbb03e252dd9dc1ad8a559c2b09867676f76de13f93b0cd6718110b3|COMPLETE" crlf))
