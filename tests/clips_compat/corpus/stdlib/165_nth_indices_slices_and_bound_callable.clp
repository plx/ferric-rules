;; #341 pinned nth$ behavior: slices-and-bound-callable
(deffunction pick (?i ?xs) (nth$ ?i ?xs))
(deffacts startup (go a b c))
(defrule exercise (go $?xs) =>
(printout t (pick 1 ?xs) ":" (pick 3 ?xs) ":" (pick 4 ?xs) crlf)
(printout t (nth$ 1 (subseq$ ?xs 2 3)) ":" (nth$ 2 (subseq$ ?xs 2 3)) ":" (nth$ 3 (subseq$ ?xs 2 3)) crlf)
)
