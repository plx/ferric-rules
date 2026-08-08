; FR-LANG-001: return unwinds exactly the current callable.
(deffunction early-return ()
  (return 1)
  2)

(defrule observe-return
  =>
  (bind ?value (early-return))
  (printout t "return=" ?value crlf)
  (assert (result ?value)))
