;; A 64-test field disjunction followed by a constrained field still loads and matches.
;; Level: boundary
;; Covers: patterns, field-disjunction, deftemplate, multifield, not, variable-binding, retract, salience
(deftemplate cell (slot a) (slot b) (slot id))
(deffacts seed
  (sym 99 x s1) (sym 0 x s2) (sym 5 z s3) (sym 7 y s4)
  (cell (a 99) (b x) (id c1)) (cell (a 0) (b x) (id c2)) (cell (a 3) (b y) (id c3))
  (seq 99 m n x q1) (seq 0 x q2)
  (phase 1))
(defrule ordered
  (sym ~0|1|2|3|4|5|6|7|8|9|10|11|12|13|14|15|16|17|18|19|20|21|22|23|24|25|26|27|28|29|30|31|32|33|34|35|36|37|38|39|40|41|42|43|44|45|46|47|48|49|50|51|52|53|54|55|56|57|58|59|60|61|62 x ?id)
  => (printout t "ordered " ?id crlf))
(defrule template
  (cell (a ~0|1|2|3|4|5|6|7|8|9|10|11|12|13|14|15|16|17|18|19|20|21|22|23|24|25|26|27|28|29|30|31|32|33|34|35|36|37|38|39|40|41|42|43|44|45|46|47|48|49|50|51|52|53|54|55|56|57|58|59|60|61|62) (b x) (id ?id))
  => (printout t "template " ?id crlf))
(defrule bound
  (sym ?v&~0|1|2|3|4|5|6|7|8|9|10|11|12|13|14|15|16|17|18|19|20|21|22|23|24|25|26|27|28|29|30|31|32|33|34|35|36|37|38|39|40|41|42|43|44|45|46|47|48|49|50|51|52|53|54|55|56|57|58|59|60|61|62 ?w&~z ?id)
  => (printout t "bound " ?id " " ?v " " ?w crlf))
(defrule sequence
  (seq ~0|1|2|3|4|5|6|7|8|9|10|11|12|13|14|15|16|17|18|19|20|21|22|23|24|25|26|27|28|29|30|31|32|33|34|35|36|37|38|39|40|41|42|43|44|45|46|47|48|49|50|51|52|53|54|55|56|57|58|59|60|61|62 $?mid x ?id)
  => (printout t "sequence " ?id " " ?mid crlf))
(defrule none
  (phase ?p)
  (not (sym ~0|1|2|3|4|5|6|7|8|9|10|11|12|13|14|15|16|17|18|19|20|21|22|23|24|25|26|27|28|29|30|31|32|33|34|35|36|37|38|39|40|41|42|43|44|45|46|47|48|49|50|51|52|53|54|55|56|57|58|59|60|61|62 x ?))
  => (printout t "none " ?p crlf))
(defrule remove-matches
  (declare (salience -5))
  ?phase <- (phase 1)
  ?s <- (sym 99 x s1)
  => (retract ?phase ?s) (assert (phase 2)) (printout t "removed s1" crlf))
(defrule complete (declare (salience -10)) => (printout t "done" crlf))
