; Harness for telefonica-clips/branches/65x/clipsjni/java-src/net/sf/clipsrules/jni/examples/router/resources/animal.clp
; Detected constructs: deffacts: MAIN::knowledge-base
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-ef111497a865dfca52f98089d8e1ce7d3383b7097e125933c5091480314b6fea-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-ef111497a865dfca52f98089d8e1ce7d3383b7097e125933c5091480314b6fea|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-ef111497a865dfca52f98089d8e1ce7d3383b7097e125933c5091480314b6fea|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-ef111497a865dfca52f98089d8e1ce7d3383b7097e125933c5091480314b6fea|COMPLETE" crlf))
