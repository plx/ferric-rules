; RH-CORE-019: named export does not expose an unexported sibling template.
(defmodule DATA (export deftemplate visible))
(deftemplate DATA::visible (slot value))
(deftemplate DATA::hidden (slot value))
(deftemplate MAIN::result (slot value))
(defrule MAIN::survivor => (printout t "survivor" crlf) (assert (result (value kept))))
