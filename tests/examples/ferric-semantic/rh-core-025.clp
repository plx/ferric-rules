; RH-CORE-025: test CEs filter joined numeric facts using boolean arithmetic conditions.
(deffacts seed (quote a 10 2) (quote b 8 5) (quote c 3 1) (budget 15))
(defrule affordable (quote ?id ?price ?discount) (budget ?budget) (test (and (> ?price 5) (<= (- ?price ?discount) ?budget))) => (printout t ?id " " (- ?price ?discount) crlf) (assert (result ?id (- ?price ?discount))))
