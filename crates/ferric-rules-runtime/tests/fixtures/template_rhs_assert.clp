(deftemplate result (slot key) (slot status (default ready)))
(defrule produce => (assert (result (key (+ 2 3)))))
(defrule observe
    (result (key ?key) (status ?status))
    =>
    (printout t ?key ":" ?status crlf))
