; Harness for rcll-refbox/src/games/llsf2014/facts.clp
; Detected constructs: deffacts: startup, light-codes, machine-specs, orders; deftemplate: machine, machine-spec, machine-light-code, puck, robot, signal, rfid-input, network-client, network-peer, attention-message, order, delivery-period, product-delivered, gamestate, exploration-report, points; defglobal: ?*M-EAST*, ?*M-NORTH*, ?*M-WEST*, ?*M-SOUTH*
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-3b86a456c92d482d30944e4ff8bfadde9fb85703e892dcac96c2b7b6c669c8e5-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-3b86a456c92d482d30944e4ff8bfadde9fb85703e892dcac96c2b7b6c669c8e5|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-3b86a456c92d482d30944e4ff8bfadde9fb85703e892dcac96c2b7b6c669c8e5|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-3b86a456c92d482d30944e4ff8bfadde9fb85703e892dcac96c2b7b6c669c8e5|COMPLETE" crlf))
