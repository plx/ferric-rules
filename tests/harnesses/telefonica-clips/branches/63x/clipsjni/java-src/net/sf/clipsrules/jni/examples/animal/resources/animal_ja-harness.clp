; Harness for telefonica-clips/branches/63x/clipsjni/java-src/net/sf/clipsrules/jni/examples/animal/resources/animal_ja.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-297175ab075ed5b693fc1af2cc96832f834b8e6ec6e37ac6c9e54c15c7b311b0-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-297175ab075ed5b693fc1af2cc96832f834b8e6ec6e37ac6c9e54c15c7b311b0|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-297175ab075ed5b693fc1af2cc96832f834b8e6ec6e37ac6c9e54c15c7b311b0|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-297175ab075ed5b693fc1af2cc96832f834b8e6ec6e37ac6c9e54c15c7b311b0|COMPLETE" crlf))
