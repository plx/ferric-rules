; RH-CORE-011: repeated reset creates exactly one initial fact activation.
(defrule bootstrap (initial-fact) => (printout t "boot" crlf) (assert (result boot)))
