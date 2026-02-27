; Harness for rcll-refbox/src/games/llsf2014/facts.clp
; Detected constructs: deffacts: startup, light-codes, machine-specs, orders; deftemplate: machine, machine-spec, machine-light-code, puck, robot, signal, rfid-input, network-client, network-peer, attention-message, order, delivery-period, product-delivered, gamestate, exploration-report, points; defglobal: ?*M-EAST*, ?*M-NORTH*, ?*M-WEST*, ?*M-SOUTH*
;
; Strategy: verify file loads and reset succeeds.
; The source file is loaded via (load ...) before this harness.

(defrule harness-verify
   (initial-fact)
   =>
   (printout t "HARNESS: loaded" crlf))
