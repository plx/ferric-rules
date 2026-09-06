; RH-CORE-001: same-name replacement after reset removes old queued activations.
(deffacts seed (left ready) (right ready))
(defrule choose (left ready) => (printout t "old" crlf) (assert (result old)))
