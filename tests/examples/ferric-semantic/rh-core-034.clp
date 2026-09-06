; RH-CORE-034: CLIPS retains valid constructs before and after an invalid construct in one load.
(defrule baseline (value ?v) => (printout t ?v crlf) (assert (result ?v)))
