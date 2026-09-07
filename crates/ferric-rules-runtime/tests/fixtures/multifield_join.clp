; A multifield join compares field types and values, not just allocation identity.
(deftemplate left (multislot values))
(deftemplate right (multislot values))
(defrule seed
    =>
    (assert (left (values alpha 2)))
    (assert (right (values alpha 2)))
    (assert (right (values alpha 2.0)))
    (assert (right (values alpha 3))))
(defrule same
    (left (values $?key))
    (right (values $?key))
    =>
    (printout t "matched " (length$ ?key) crlf)
    (assert (matched)))
